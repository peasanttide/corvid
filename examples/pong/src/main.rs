//! Plays pong: alone, against another process, or against another peer in this
//! one.
//!
//! ```text
//! pong                                       one seat, a window, nobody in the other seat
//! pong --seat 1                              the other paddle
//! pong --demo                                two peers over a lying link, headless, one table
//! pong --together                            two peers in this process, seat 0 in a window
//! pong --listen 9000 --connect HOST:9001     two machines, over a socket
//! ```
//!
//! Everything `corvid` already accepts — `--headless`, `--ticks N`,
//! `--capture DIR`, `--replay FILE`, `--load N` — is accepted too, because the
//! flags below are taken out of the command line and the rest is handed to
//! [`corvid::Arguments`] unchanged.

use std::io::Write;

use corvid::{App, Arguments, Error, Input};

use corvid::PlayerId;
use pong::{Ears, Graphics, Hands, RATE, Table};

/// What this binary was asked to do, beyond what `corvid` understands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Ours {
    /// Which seat this machine plays.
    seat: u16,
    /// Print the two-peer table and exit, playing no window.
    demo: bool,
    /// Play both seats in this process, showing seat zero.
    together: bool,
    /// Drive this seat's paddle from a script rather than from a keyboard.
    bot: bool,
    /// The port to bind, for a run over a socket.
    listen: Option<u16>,
    /// Where the other machine is.
    connect: Option<String>,
    /// What is left for `corvid` to read.
    rest: Vec<String>,
}

/// The usage this binary adds to `corvid`'s own.
const USAGE: &str = "\
pong: [--seat N] [--demo] [--together] [--listen PORT] [--connect HOST:PORT]

  --seat N          which paddle this machine plays: 0 defends the left, 1 the
                    right
  --demo            play two peers against each other over a link that loses
                    and delays packets, print what the netcode did, and exit
  --together        play both seats in this process over the same lying link,
                    showing seat 0's window — the whole of multiplayer with one
                    command
  --listen PORT     bind this UDP port
  --connect ADDR    the other machine, as HOST:PORT
  --bot             move this paddle from a script instead of a keyboard, which
                    is what makes a headless run against another process an
                    actual rally

and everything corvid takes as well; run with --help for that list.";

fn main() -> corvid::Result {
    corvid::watch();

    let ours = ours(std::env::args().skip(1))?;

    if ours.demo {
        return demo();
    }
    if ours.together {
        return together(&ours);
    }

    let mut arguments = Arguments::parse(ours.rest.iter().cloned()).map_err(usage)?;
    // Taken rather than forwarded: `App::run` applies `--headless` by *undoing*
    // whichever backend was asked for, and this binary wants a third one — a
    // run with an adapter and no window, so that a capture with no window in it
    // still has a picture in it. Every other flag goes through untouched.
    let headless = std::mem::take(&mut arguments.headless);
    let app = App::<Table, Hands, Graphics, Ears>::new()
        .opening(pong::opening())
        .rate(RATE)
        .seat(PlayerId(ours.seat))
        .input(Input::new(pong::action::SETS))
        // A paddle that moves without a keyboard. The pattern is a function of
        // the tick alone, so both machines' actions are decided before either
        // runs and the session's outcome is the netcode's rather than the
        // scheduler's — and it changes direction often enough that each peer's
        // prediction of the other is wrong several times a second, which is the
        // point.
        //
        // A controller's configuration rather than a feed of input snapshots:
        // answering with an action per tick is what a controller is for, and a
        // scripted player is one whose answer does not depend on a device.
        .controls(ours.bot.then_some(ours.seat));

    #[cfg(feature = "net")]
    let app = match socket(&ours)? {
        Some(transport) => app.transport(transport),
        None => app,
    };

    // The backend, which is this binary's decision and not a flag's: a window
    // for a player, an adapter drawing into a texture for a run that is being
    // recorded with nobody watching, and neither for everything else.
    let app = match (headless, arguments.capture.is_some()) {
        (true, true) => app.offscreen(OFFSCREEN),
        (true, false) => app.headless(),
        #[cfg(feature = "window")]
        (false, _) => app.window().bindings(pong::action::bindings()),
        #[cfg(not(feature = "window"))]
        (false, _) => app,
    };

    let outcome = app.arguments(arguments).run()?;
    if headless {
        report(&outcome)?;
    }
    Ok(())
}

/// Takes this binary's own flags out of the command line.
///
/// Everything it does not recognise is left for [`Arguments`], which is what
/// keeps `--ticks` and `--capture` working here without this function knowing
/// what they mean.
fn ours(arguments: impl Iterator<Item = String>) -> Result<Ours, Error> {
    let mut ours = Ours::default();
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--seat" => {
                let value = arguments.next().unwrap_or_default();
                ours.seat = value.parse().map_err(|_| {
                    usage(corvid::Argument::NotANumber {
                        flag: "--seat",
                        value,
                    })
                })?;
            }
            "--demo" => ours.demo = true,
            "--bot" => ours.bot = true,
            "--together" => ours.together = true,
            "--listen" => {
                let value = arguments.next().unwrap_or_default();
                ours.listen = Some(value.parse().map_err(|_| {
                    usage(corvid::Argument::NotANumber {
                        flag: "--listen",
                        value,
                    })
                })?);
            }
            "--connect" => ours.connect = arguments.next(),
            other => ours.rest.push(other.to_owned()),
        }
    }
    Ok(ours)
}

/// This binary's usage, in front of `corvid`'s.
fn usage(why: corvid::Argument) -> Error {
    let mut out = std::io::stderr();
    drop(writeln!(out, "{USAGE}"));
    Error::Argument(why)
}

/// The socket, for a run that named one.
#[cfg(feature = "net")]
fn socket(ours: &Ours) -> Result<Option<Box<dyn corvid_net::Transport>>, Error> {
    let (Some(port), Some(peer)) = (ours.listen, ours.connect.as_deref()) else {
        return Ok(None);
    };
    let udp = corvid_net::udp::UdpNet::bind(("0.0.0.0", port), corvid_net::PeerId(ours.seat))
        .map_err(|why| socket_error("this port could not be bound", &why))?;
    udp.connect(corvid_net::PeerId(1 - ours.seat), peer)
        .map_err(|why| socket_error("that address could not be reached", &why))?;
    Ok(Some(Box::new(udp)))
}

/// A socket that would not open, as the error this binary hands back.
#[cfg(feature = "net")]
fn socket_error(what: &str, why: &std::io::Error) -> Error {
    Error::Wrote {
        path: std::path::PathBuf::from(what),
        why: std::io::Error::new(why.kind(), why.to_string()),
    }
}

/// Two peers over a lying link, and what the netcode did about it.
///
/// The same [`Match`](pong::rally::Match) the tests drive, with a table printed
/// instead of assertions — so what this prints is measured rather than
/// described.
#[cfg(feature = "net")]
fn demo() -> corvid::Result {
    use corvid_net::Schedule;
    use pong::rally::{Match, SEED, agreed, chase};

    tracing::info!(
        ticks = TICKS,
        hz = RATE.hz(),
        seed = format_args!("{SEED:#x}"),
        "two peers over a lying link",
    );

    for (name, schedule) in [
        ("perfect", Schedule::PERFECT),
        ("domestic", Schedule::DOMESTIC),
        ("mobile", Schedule::MOBILE),
    ] {
        let mut playing = Match::new(schedule, SEED, [chase, chase]).map_err(Error::Shape)?;
        playing.play(TICKS).map_err(halted)?;
        let traces = playing.traces();
        let line = agreed(traces);
        // Compared over every tick both peers have every seat's real action
        // for, which is what `agreed` answers.
        let compared = usize::try_from(line.0).unwrap_or(usize::MAX);
        let same = traces.windows(2).all(|pair| {
            pair[0].marks.get(..=compared) == pair[1].marks.get(..=compared)
                && pair[0].marks.len() > compared
        });
        let Some(first) = traces.first() else {
            continue;
        };
        // A `warn` when the peers disagreed, because that is the one outcome
        // here that is a defect rather than a measurement — and a level is how
        // a reader is told which is which without reading the row.
        if same {
            tracing::info!(
                link = name,
                ticks = first.tick.0,
                confirmed = line.0,
                heard = first.heard,
                rollbacks = first.rollbacks,
                deepest = first.deepest,
                replayed = first.resimulated,
                stalls = first.stalls,
                "peers agreed",
            );
        } else {
            tracing::warn!(
                link = name,
                ticks = first.tick.0,
                confirmed = line.0,
                "peers disagreed",
            );
        }
    }
    Ok(())
}

/// The demo, on a build with no netcode compiled in.
#[cfg(not(feature = "net"))]
fn demo() -> corvid::Result {
    tracing::warn!("this build has no `net` feature, so there is no netcode to demonstrate");
    Ok(())
}

/// Both seats in this process, over the same lying link, with seat zero shown.
#[cfg(all(feature = "net", feature = "window"))]
fn together(ours: &Ours) -> corvid::Result {
    let arguments = Arguments::parse(ours.rest.iter().cloned()).map_err(usage)?;
    let outcome = pong::rally::together(
        PlayerId(ours.seat),
        RATE,
        arguments.ticks,
        !arguments.headless,
    )?;
    if arguments.headless {
        report(&outcome)?;
    }
    Ok(())
}

/// The same, on a build that cannot open a window or has no netcode.
#[cfg(not(all(feature = "net", feature = "window")))]
fn together(_ours: &Ours) -> corvid::Result {
    tracing::warn!("this build has no window or no netcode, so there is nothing to play together");
    Ok(())
}

/// A peer that could not carry on, as this binary's error.
#[cfg(feature = "net")]
fn halted(why: corvid_lockstep::Halt) -> Error {
    match why {
        corvid_lockstep::Halt::Desync(desync) => Error::Diverged(Box::new(desync)),
        other => Error::Halted(Box::new(other)),
    }
}

/// How far back the reported digest is taken from.
///
/// Past `Budget::DEFAULT`'s eight ticks ahead and two of delay, with room: a
/// state that far back was computed from actions every seat really submitted.
const SETTLED: u64 = 20;

/// What the netcode did, for a run that had any.
///
/// Empty for a single-seat run, which heard nothing and rolled back never — so
/// a run with no network prints exactly what it printed before there was one.
#[cfg(feature = "net")]
fn netcode(outcome: &corvid::Outcome<Table>) -> String {
    let played = outcome.traffic;
    if played.heard == 0 && played.sent == 0 {
        return String::new();
    }
    format!(
        "
{} heard, {} sent, {} rollbacks over {} replayed ticks (deepest {}), {} stalls",
        played.heard,
        played.sent,
        played.rollbacks,
        played.resimulated,
        played.deepest,
        played.stalls,
    )
}

/// The same, on a build with no netcode compiled in.
#[cfg(not(feature = "net"))]
fn netcode(_outcome: &corvid::Outcome<Table>) -> String {
    String::new()
}

/// How big a headless capture draws.
///
/// Sixteen by nine at a size a golden can be compared at without being a
/// megabyte of PNG per frame.
const OFFSCREEN: corvid::Extent = corvid::Extent::new(640, 360);

/// How long the demo plays.
#[cfg(feature = "net")]
const TICKS: u64 = 900;

/// What a headless run says when it stops.
fn report(outcome: &corvid::Outcome<Table>) -> corvid::Result {
    let mut out = std::io::stdout();
    let table = &outcome.state;
    // **Not the last tick's digest.** A run stops when it stops, and the newest
    // few ticks of a peer's state were simulated partly from predictions of
    // what the other machine did — so two processes that stopped a second apart
    // print different numbers for the same session, which is prediction working
    // rather than anything disagreeing. The settled one is a tick far enough
    // back that every seat's real action was in hand, and *that* is the number
    // two peers can be held to.
    let settled = corvid::Tick(table.now.0.saturating_sub(SETTLED));
    let mark = outcome.session.marks.get(settled).map_or_else(
        || "unknown".to_owned(),
        |mark| format!("{:#018x}", mark.to_u64()),
    );
    writeln!(
        out,
        "tick {} — {} : {} — digest {mark} at tick {}{}{}",
        table.now.0,
        table.scores[0],
        table.scores[1],
        settled.0,
        table
            .over
            .map_or_else(String::new, |seat| format!(", seat {seat} won")),
        netcode(outcome),
    )
    .and_then(|()| out.flush())
    .map_err(|why| Error::Wrote {
        path: std::path::PathBuf::from("stdout"),
        why,
    })
}
