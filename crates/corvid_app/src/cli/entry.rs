//! The `main` a game writes, and the two ways a process stops.
//!
//! The seam is the process: this is the only file in the crate that exits one
//! or writes to a stream, which is what lets every other file stay a library.

use crate::app::{App, Outcome};
use crate::cli::{Argument, Arguments};
use corvid_control::Controller as _;
use corvid_replay::Opens;
use corvid_time::Tick;

use crate::cli::watch;
use crate::game::{AuralizerConfig, BotConfig, ControllerConfig, Game, RenderConfig};

/// Plays the game the process was started to play.
///
/// **This is the whole of a Corvid `main`, and there is no second spelling.**
///
/// ```no_run
/// # use core::convert::Infallible;
/// # use std::sync::Arc;
/// # use corvid_behavior::{Level, State};
/// # use corvid_replay::{Opening, Opens, Profile, Schema, Seed};
/// # use corvid_time::Tick;
/// # use serde::{Deserialize, Serialize};
/// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// # struct Nowhere;
/// # impl Level for Nowhere {
/// #     type Error = Infallible;
/// #     fn load(_: &str) -> Result<Self, Infallible> { Ok(Self) }
/// # }
/// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// # struct Server;
/// # impl State for Server {
/// #     const NAME: &'static str = "server";
/// #     type Level = Nowhere;
/// #     type Rules = ();
/// #     type Action = ();
/// # }
/// # impl corvid_replay::Opens for Server {
/// #     fn opening() -> corvid_replay::Opening<Self> { unimplemented!() }
/// # }
/// /// A game that draws nothing, hears nothing and reads no device says so in
/// /// four lines: `()` is a controller that submits the idle action forever, a
/// /// renderer that opens no adapter and an ear that opens no sound card.
/// #[derive(Debug)]
/// struct Dedicated;
///
/// impl corvid_app::Game for Dedicated {
///     const PERIOD: corvid_time::TickSpan = corvid_time::TickSpan::CRADLE;
///     type State = Server;
///     type Controller = ();
///     type Bot = ();
///     type Render = ();
///     type Auralizer = ();
/// }
///
/// fn main() {
///     corvid_app::main::<Dedicated>()
/// }
/// ```
///
/// A window, a headless run, a recording, a save slot, a bot and another
/// machine are all the same program: this reads the process's arguments and
/// decides. **A game never asks for determinism**, because a game that had to
/// call `.headless()` would have a mode that is deterministic and a mode that
/// is not, and only one of them would be tested. [`Arguments`] is the list of
/// what an operator may say.
///
/// # What it does with each of them
///
/// The opening is the game's own [`Opens::opening`], the input declaration is
/// [`Controller::SETS`](corvid_control::Controller::SETS) and the binding table is
/// [`Controller::bindings`](corvid_control::Controller::bindings) -- the three things only the game can state.
/// Everything else on the [`App`](crate::App) comes from the command line, through
/// [`App::arguments`], so an operator's flag beats a default whichever order
/// they were written in.
///
/// # Which backend this picks
///
/// `--headless` is no window, no adapter and no audio device. Without it, a
/// build with the `window` feature opens a window and a build without one plays
/// headless, because a device drawing frames nothing will look at is a device
/// doing nothing. Every build can play headless, so `--headless` means the same
/// thing in all of them -- which is what lets a script pass it without knowing
/// which build it is talking to. A harness that wants an adapter and no window
/// builds its own [`App`](crate::App) and calls
/// [`offscreen`](crate::App::offscreen).
///
/// # The bound, and the game that has nothing to draw
///
/// `G: Game`, which is the five types a game is. There is no configuration in
/// which the bound is weaker: a game that reaches this has a renderer and an
/// ear whether it draws or sounds or not.
///
/// `type Render = ();` is the one line that satisfies it for a game with
/// nothing to draw: a dedicated server, a determinism check, or a game that has
/// not drawn anything yet writes that line and writes no `wgpu`. The run opens
/// no adapter and never calls `draw`, so the line costs what it says it does.
///
/// # What it does with a command line
///
/// An operator who asked for the usage got what they asked for: `--help` writes
/// it to **stdout** and the process exits zero, so a shell script does not have
/// to special-case it. [`Arguments::parse`] still reports it as
/// [`Argument::Help`], because that function may not print.
///
/// A command line that could not be acted on writes the reason and the usage to
/// **stderr** and stops the process with status 2. A run that started and could
/// not finish writes its reason to the same stream and stops with status 1. So
/// **this function hands nothing back**: every answer it has is one an operator
/// reads, and each is the error's own [`Display`](core::fmt::Display) rather than the
/// [`Debug`] that a `main` returning `Result` would have printed -- the
/// difference between `Wrote { path: "...", why: Os { code: 13, .. } }` and the
/// sentence [`Error`](crate::Error) wrote for that case.
///
/// This module is the only one in the crate that writes to the process's
/// streams at all, and it does so through `println!` and `eprintln!` under
/// stated exceptions rather than through handles that would reach the same
/// streams while passing the lint. A harness that wants none of that drives
/// [`App::launch`], which reads the same command line and hands every one of
/// these back without writing anything.
///
/// # What it does with the exit code
///
/// Three numbers, and they are three different questions answered:
///
/// | | |
/// |---|---|
/// | 0 | the run finished, or `--help` was asked for |
/// | 1 | the run started and could not finish |
/// | 2 | the command line could not be acted on |
///
/// Two is the conventional status for a usage error, and one is what any `Err`
/// out of a `main` collapses to anyway -- so keeping it for the run means a
/// script that already told "the run broke" from "I typed it wrong" keeps
/// telling them apart, and neither is confusable with the other.
///
/// Above those, a [`quit`](corvid_behavior::Command::quit) names a status of its
/// own, and a status that is not
/// [`SUCCESS`](corvid_behavior::ExitCode::SUCCESS) leaves the process with that
/// number. Everything a run writes down has been written by then: a recording is
/// closed and a save is on disk before the run hands its outcome back.
pub fn main<G: Game>()
where
    ControllerConfig<G>: Default,
    BotConfig<G>: Default,
    RenderConfig<G>: Default,
    AuralizerConfig<G>: Default,
{
    // Before anything, so that a refusal to parse the command line is itself
    // reportable.
    watch();
    let Some(arguments) = command_line() else {
        return;
    };
    // Whether the operator asked for a run with no devices, which is the one
    // thing that decides whether the digest goes to stdout. Read here because
    // the arguments are handed to the app below.
    let headless = arguments.headless;

    // The three things the game states and no flag can: where a session starts,
    // which actions exist, and which control raises which. `Controller::SETS`
    // is required rather than defaulted because a run with an empty
    // declaration binds no key and no axis and answers `RELEASED` to every
    // query for the length of the run.
    let app = App::<G>::new()
        .opening(<G::State as Opens>::opening())
        .input(corvid_input::Input::new(G::Controller::SETS));

    #[cfg(feature = "net")]
    let app = match (arguments.listen, arguments.connect.as_deref()) {
        (Some(port), Some(peer)) => match crate::net::udp(port, arguments.seat, peer) {
            Ok(transport) => app.transport(transport),
            // A seat these two flags cannot arrange, which is a command line
            // rather than a run: same stream, same status as any other.
            Err(crate::Error::Argument(why)) => refuse(&why),
            Err(why) => failed(&why),
        },
        // Neither flag, which is a run that plays alone. Half of a link does not
        // reach here: `Arguments::parse` refuses `--listen` without `--connect`
        // and the reverse, so the only way to arrive with one of them is an
        // `Arguments` a harness filled in by hand, and a harness that did that
        // asked for a run alone.
        _ => app,
    };

    // The table is a windowed run's business alone: a run with no window reads
    // no device, and `App::bindings` holds a table it would have nothing to
    // resolve against.
    #[cfg(feature = "window")]
    let app = if headless {
        app
    } else {
        app.window().bindings(G::Controller::bindings())
    };

    let outcome = match app.arguments(arguments).run() {
        Ok(outcome) => outcome,
        // A `--level` this game cannot open on is a command line that could not
        // be acted on, noticed where the game is known rather than in the
        // parser -- so it gets the answer every other refused command line gets,
        // on the same stream and with the same status, instead of arriving as a
        // failed run.
        Err(crate::Error::Argument(why)) => refuse(&why),
        Err(why) => failed(&why),
    };
    finish::<G>(&outcome, headless);
}

/// What a process that could not read its command line exits with.
///
/// Two, which is the conventional status for a usage error and is deliberately
/// not [`FAILED`], so that a run that failed for a reason that is not the
/// command line stays distinguishable from a command line nobody could act on.
const REFUSED: i32 = 2;

/// What a process whose run could not finish exits with.
///
/// One, which is what any `Err` handed back from a `main` collapses to -- so a
/// script that reads a status rather than a stream sees the number it expects
/// for a program that failed, and the sentence saying why is on stderr for a
/// reader who wants it.
const FAILED: i32 = 1;

/// Reads the process's arguments, and answers whatever they asked for that is
/// not a game.
///
/// [`None`] is "the usage was asked for and has been written, and there is no
/// game left to play" -- a success, and the process exits zero.
///
/// A command line that could **not** be acted on is written to stderr and the
/// process stops with [`REFUSED`], rather than travelling back as an `Err`.
/// Both halves of that are the point. The message is
/// [`Argument`]'s [`Display`](core::fmt::Display) -- the sentence and the usage under
/// it -- where an `Err` out of a `main` is printed by the runtime with
/// [`Debug`], which would show an operator `Argument(Conflicting { flags:
/// [...] })` and no list of what the runtime accepts. And the status is 2
/// rather than the [`FAILED`] a run that broke leaves.
///
/// It is the same shape the `--help` arm has, one stream over: this crate's
/// `main` is a program, and a program answers a command line it was given.
/// [`App::launch`] is the library half -- it hands [`Error::Argument`](crate::Error::Argument) back and
/// writes nothing -- which is what a harness driving a run by hand wants.
fn command_line() -> Option<Arguments> {
    match Arguments::from_env() {
        Ok(arguments) => Some(arguments),
        Err(Argument::Help) => {
            #[expect(
                clippy::print_stdout,
                reason = "the same exception `finish` carries, and the other half of it: an operator who passed `--help` asked for this text on stdout. Writing it to an `io::stdout()` handle instead passed the lint while doing the identical thing"
            )]
            {
                println!("{}", Arguments::USAGE);
            }
            None
        }
        Err(why) => refuse(&why),
    }
}

/// Says why a command line could not be acted on, and stops the process.
///
/// The one place either kind of refusal is answered -- the parser's, and the
/// `--level` that only the game could judge -- so an operator gets the same
/// sentence, on the same stream, with the same status, wherever it was noticed.
///
/// Nothing is open when this is reached: no window, no adapter, no capture
/// directory, no file. So there is nothing here that unwinding would close and
/// exiting will not.
fn refuse(why: &Argument) -> ! {
    #[expect(
        clippy::print_stderr,
        reason = "one of the two writes this crate makes to stderr, and it is here for the reason every write in this file is: a program that was handed a command line it cannot act on says so where an operator reads it. stderr rather than stdout, because a program's own answer belongs alone on stdout for a pipe"
    )]
    {
        eprintln!("{why}");
    }
    std::process::exit(REFUSED);
}

/// Says why a run could not finish, and stops the process.
///
/// [`Error`](crate::Error) writes a sentence for every one of its variants --
/// which file could not be written and why, which port would not bind, which
/// tick two peers disagreed at -- and **not one of those sentences reaches an
/// operator through a `main` that hands the error back**, because the runtime
/// prints a returned `Err` with [`Debug`]. So this prints the
/// [`Display`](core::fmt::Display), for the same reason and on the same stream
/// [`refuse`] does, one status down.
///
/// Nothing this process opened is still open when this is reached, so exiting
/// closes nothing that unwinding would have: [`App::run`](crate::App::run) takes its app by
/// value, and a window, an adapter, a recording and a capture directory are all
/// dropped inside it before the `Err` comes back.
fn failed(why: &crate::Error) -> ! {
    #[expect(
        clippy::print_stderr,
        reason = "the other write to stderr, and the same exception: a program whose run could not finish says why where an operator reads it, rather than handing back an `Err` the runtime renders with `Debug`"
    )]
    {
        eprintln!("{why}");
    }
    std::process::exit(FAILED);
}

/// How far back a reported digest is taken from.
///
/// Past a [`Budget::DEFAULT`](corvid_lockstep::Budget)'s eight ticks ahead and
/// two of delay, with room. The newest few ticks of a peer's state were
/// simulated partly from predictions of what another machine did, so two
/// processes that stopped a second apart report different numbers for the same
/// session -- which is prediction working rather than anything disagreeing. A
/// state this far back was computed from actions every seat really submitted,
/// so it is the number two peers can be held to.
#[cfg(feature = "net")]
const SETTLED: u64 = 20;
/// A run with nobody else in it predicts nothing, so its last tick is settled.
#[cfg(not(feature = "net"))]
const SETTLED: u64 = 0;

/// Reports where the run got to, and leaves the process with the status a
/// [`quit`](corvid_behavior::Command::quit) named.
///
/// The report is a `tracing` event, so a game that wants it structured installs
/// a subscriber -- [`watch`] is what a `main` calls to get one. A **headless**
/// run also writes the settled digest to stdout, alone on the line: that is
/// what an operator asked for by passing `--headless`, and it is the one thing
/// here a script wants, so a score or a counter beside it would be something
/// every consumer has to parse past. A windowed run prints nothing, because the
/// digest is not what somebody watching a window came for.
///
/// The stdout line is a `println!` under a named exception rather than a write
/// to a handle. Both reach the same stream; only one of them says so where a
/// reader looking for the workspace's printing rule will find it.
fn finish<G: Game>(outcome: &Outcome<G>, headless: bool) {
    let last = outcome.session.last();
    let settled = Tick(last.0.saturating_sub(SETTLED));
    let mark = outcome.session.marks.get(settled).map_or_else(
        || "unknown".to_owned(),
        |mark| format!("{:#018x}", mark.to_u64()),
    );

    tracing::info!(
        name: "corvid_app.finished",
        tick = %last,
        settled = settled.0,
        digest = %mark,
        requests = outcome.requests.len(),
        "the run ended",
    );
    #[cfg(feature = "net")]
    if outcome.traffic.heard != 0 || outcome.traffic.sent != 0 {
        tracing::info!(
            name: "corvid_app.netcode",
            heard = outcome.traffic.heard,
            sent = outcome.traffic.sent,
            rollbacks = outcome.traffic.rollbacks,
            resimulated = outcome.traffic.resimulated,
            deepest = outcome.traffic.deepest,
            stalls = outcome.traffic.stalls,
            "what the link cost",
        );
    }

    if headless {
        #[expect(
            clippy::print_stdout,
            reason = "this crate's `main` is a program rather than a library: an operator who passed `--headless` asked for this line on stdout, and a `main` of one line has nowhere to install a subscriber. Writing to an `io::stdout()` handle instead would pass the lint while doing the identical thing, which is worse -- the exception belongs where a reader can see it"
        )]
        {
            println!("{mark}");
        }
    }

    if outcome.exit != corvid_behavior::ExitCode::SUCCESS {
        // The run is over and everything it writes down is written: `App::run`
        // closes a recording before it hands an outcome back. What is left to
        // do is hand the operating system the number the game asked for, and a
        // `main` returning `Result` cannot -- every `Err` from one is status 1.
        std::process::exit(i32::from(outcome.exit.0));
    }
}
