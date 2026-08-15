//! What every Corvid game answers to, and the parser that reads it.
//!
//! The seam against `argument.rs` is success: everything here is the shape a
//! command line has once it parses, and the refusals are next door.

use std::path::PathBuf;

use corvid_behavior::{PlayerId, SaveSlot};
use corvid_time::Ticks;

use crate::cli::Argument;

/// What the run opens on.
///
/// Three ways of naming a starting point and one field to hold whichever was
/// given, because they are one decision: a run opens on a level, on a slot or
/// on a recording, and a command line that named two of them named two runs.
/// [`Argument::Conflicting`] is what that is.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Load {
    /// The name of a level, as
    /// [`Level::load`](corvid_behavior::Level::load) reads one.
    ///
    /// Kept as text because a level name *is* text: this parser knows no game,
    /// and the game's own loader is the only thing that can say whether the
    /// name means anything. [`App::run`](crate::App::run) is what hands it over, and a name that
    /// loader refuses is [`Argument::UnreadableLevel`] there.
    Level(String),
    /// A save slot.
    Save(SaveSlot),
    /// A recorded session, which is what `--record` wrote.
    Demo(PathBuf),
}

/// What every Corvid game answers to.
///
/// [`main`](crate::main) reads these from the process's arguments and decides
/// the whole shape of the run from them, which is why a game's `main` is one
/// line and why a game never calls [`headless`](crate::App::headless): a game
/// that had to ask for determinism would have a mode that is deterministic and
/// a mode that is not, and only one of them would be tested.
/// [`App::launch`](crate::App::launch) is the same reading, for a harness that
/// is driving a run by hand.
///
/// # The surface, and why it is this small
///
/// | | |
/// |---|---|
/// | `--headless` | play with no window, no adapter and no audio device |
/// | `--spectator` | claim no seat: submit nothing, and watch the one this client would have played |
/// | `--bots N` | let the game's bot play `N` seats nobody is in |
/// | `--ticks N` | stop once `N` ticks have run, counted from where the run opened |
/// | `--level NAME` | open on this level rather than on the game's own |
/// | `--load N` | open on save slot `N` |
/// | `--demo FILE` | open on the session in `FILE`, and carry it on |
/// | `--record FILE` | write the session to `FILE` as the run plays |
/// | `--state DIR` | put this game's saves, settings and bindings under `DIR` |
/// | `--seat N` | which seat this machine plays |
/// | `--listen PORT --connect HOST:PORT` | the socket the other machine is behind, and either without the other is [`Argument::Incomplete`] |
/// | `--help`, `-h` | write the usage to stdout and stop, successfully |
///
/// Every one of them is a thing the *operator* decides rather than the game:
/// whether this machine has a display, who is at the controls, how long to run
/// for, where the run's files live, what to open on and who else is in it. A
/// setting only the game can know -- its rules, its passes, its opening -- is not
/// here and should not be, because a flag for it would be a flag whose legal
/// values only the game could list. `--level` is the near miss and the reason
/// it works: what it carries is a name, and
/// [`Level::load`](corvid_behavior::Level::load) is what decides whether the
/// game has one -- so this parser holds a string and the game is what reads it.
///
/// `--ticks 100` and `--ticks=100` are the same argument. A flag that takes a
/// value and is given none is [`Argument::Missing`] rather than a default,
/// because a run of "0 ticks" and a run of "as long as you like" are both things
/// somebody might have meant.
///
/// # Why no argument-parsing library
///
/// Eleven flags, no subcommands, no completion, and a workspace habit of owning
/// small things. The whole of the parser below is shorter than the manifest
/// entry and the feature audit a dependency would cost, and it is what the
/// crate's public error type has to describe anyway.
///
/// ```
/// use corvid_app::{Arguments, Load};
/// use corvid_behavior::{PlayerId, SaveSlot};
/// use corvid_time::Ticks;
///
/// let arguments = Arguments::parse(["--headless", "--ticks=90"])?;
/// assert!(arguments.headless);
/// assert_eq!(arguments.ticks, Some(Ticks(90)));
/// assert_eq!(arguments.record, None);
///
/// // The three ways of opening are one field, because they are one decision.
/// let resumed = Arguments::parse(["--load", "3", "--state", "here/"])?;
/// assert_eq!(resumed.load, Some(Load::Save(SaveSlot(3))));
/// assert_eq!(resumed.state.as_deref(), Some(std::path::Path::new("here/")));
///
/// // And a command line that named two of them named two runs.
/// assert!(Arguments::parse(["--load", "3", "--demo", "run/session"]).is_err());
/// # Ok::<(), corvid_app::Argument>(())
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Arguments {
    /// Open no window, no adapter and no audio device.
    pub headless: bool,
    /// Claim no seat: submit nothing, and watch the seat
    /// [`seat`](Self::seat) names, which is the first one unless it says
    /// otherwise.
    pub spectator: bool,
    /// How many unclaimed seats the game's bot plays.
    pub num_bots: u16,
    /// Stop once this many ticks have run, counted from where the run opened.
    pub ticks: Option<Ticks>,
    /// What to open on, rather than the game's own opening.
    pub load: Option<Load>,
    /// Where to write the session, so that `--demo` can open it again.
    pub record: Option<PathBuf>,
    /// Where this game's files live, rather than the user data dir.
    pub state: Option<PathBuf>,
    /// Which seat this machine plays.
    pub seat: PlayerId,
    /// The UDP port to bind.
    ///
    /// One half of a link, and [`parse`](Self::parse) refuses it without the
    /// other: a port bound with nobody to reach is a run that waits for a
    /// datagram nothing will send.
    pub listen: Option<u16>,
    /// Where the other machine is, as `HOST:PORT`.
    ///
    /// The other half, refused alone for the matching reason: an address with no
    /// socket behind it has nowhere for the answer to arrive.
    pub connect: Option<String>,
}

/// What a game is told it accepts.
///
/// A `&'static str` rather than something printed, because a parser that
/// reached for the process's streams is a parser nobody can redirect or test.
/// [`main`](crate::main) is the one place that writes it -- to stdout, and with
/// a zero status, because asking for the usage is not a failure.
/// [`parse`](crate::Arguments::parse) reports [`Argument::Help`] instead, whose
/// [`Display`](core::fmt::Display) is this text, which is what a harness driving a run
/// by hand sees. That split is the reason this text is public at all: a parser
/// that may not print and a `main` that may are two different jobs, and only one
/// of them is allowed the stream.
pub(super) const USAGE: &str = "\
corvid: [--headless] [--spectator] [--bots N] [--ticks N]
        [--level NAME | --load N | --demo FILE] [--record FILE] [--state DIR]
        [--seat N] [--listen PORT --connect HOST:PORT]

  --headless        play with no window, no adapter and no audio device
  --spectator       claim no seat: submit nothing, and watch the seat this
                    machine would have played
  --bots N          let the game's bot play N seats nobody is in
  --ticks N         stop once N ticks have run, counted from where the run
                    opened
  --level NAME      open on this level rather than the game's own
  --load N          open on save slot N
  --demo FILE       open on the session in FILE, which is what --record wrote,
                    and carry it on
  --record FILE     write the session to FILE as the run plays
  --state DIR       put this game's saves, settings and bindings under DIR
                    rather than the user data dir
  --seat N          which seat this machine plays
  --listen PORT     bind this UDP port; goes with --connect, and either
                    without the other is refused
  --connect ADDR    the other machine, as HOST:PORT
  --help, -h        this";

impl Arguments {
    /// What a game is told it accepts, for a `main` that wants to print it.
    pub const USAGE: &'static str = USAGE;

    /// Reads the arguments this process was started with.
    ///
    /// The first is the program's own name and is skipped, which is the one
    /// thing this does that [`parse`](Self::parse) does not -- and the reason
    /// they are two functions is that a test cannot choose what
    /// [`std::env::args`] answers.
    ///
    /// # Errors
    ///
    /// Whatever [`parse`](Self::parse) reports.
    pub fn from_env() -> Result<Self, Argument> {
        Self::parse(std::env::args().skip(1))
    }

    /// Reads arguments from anything that yields them, program name already
    /// removed.
    ///
    /// # Errors
    ///
    /// [`Argument::Help`] if help was asked for, which is not a failure and is
    /// reported as one because this function may not print -- [`main`](crate::main)
    /// writes [`USAGE`](Self::USAGE) to stdout for it and exits zero;
    /// [`Argument::Unknown`] for a flag this does not have,
    /// [`Argument::Missing`] for one whose value is absent,
    /// [`Argument::Unexpected`] for a value on a flag that takes none,
    /// [`Argument::NotANumber`] for a count that is not one,
    /// [`Argument::Conflicting`] for two flags that cannot both be acted on, and
    /// [`Argument::Incomplete`] for one that means nothing without another.
    pub fn parse<I, S>(arguments: I) -> Result<Self, Argument>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut parsed = Self::default();
        let mut arguments = arguments.into_iter().map(Into::into);

        // Which of the three ways of opening was written, so that a second one
        // is refused naming both rather than quietly winning.
        let mut opened: Option<&'static str> = None;
        // Where `--bots` and `--connect` were written, for the same refusal:
        // the two are checked at the end, because either order is a command
        // line somebody typed and the message names them in the order they
        // typed them.
        let (mut botted, mut peered) = (None, None);
        let mut position = 0_usize;

        while let Some(argument) = arguments.next() {
            position += 1;
            // `--flag=value` and `--flag value` are the same argument. The split
            // is on the first `=` so that a path with one in it survives being
            // passed as `--record=a=b`.
            let (flag, attached) = match argument.split_once('=') {
                Some((flag, value)) => (flag.to_owned(), Some(value.to_owned())),
                None => (argument, None),
            };

            /// The value for a flag that takes one, from `=` or from the next
            /// argument.
            macro_rules! value {
                ($name:literal) => {
                    match attached {
                        Some(value) => value,
                        None => arguments.next().ok_or(Argument::Missing { flag: $name })?,
                    }
                };
            }

            /// Refuses a value on a flag that takes none, which is otherwise a
            /// `--headless=false` that turns it on.
            macro_rules! bare {
                ($name:literal) => {
                    if attached.is_some() {
                        return Err(Argument::Unexpected { flag: $name });
                    }
                };
            }

            /// A number, or the flag that was not given one.
            macro_rules! number {
                ($name:literal) => {{
                    let value = value!($name);
                    value
                        .parse()
                        .map_err(|_| Argument::NotANumber { flag: $name, value })?
                }};
            }

            /// The one opening field, refusing a second way of filling it.
            ///
            /// The same flag twice is the ordinary "the later one wins" every
            /// other flag here has: two `--load`s are one command line that
            /// changed its mind, where a `--load` and a `--demo` are two
            /// different runs and neither of them is the one that was asked
            /// for.
            macro_rules! open {
                ($name:literal, $what:expr) => {{
                    if let Some(first) = opened.filter(|first| *first != $name) {
                        return Err(Argument::Conflicting {
                            flags: [first, $name],
                        });
                    }
                    opened = Some($name);
                    parsed.load = Some($what);
                }};
            }

            match flag.as_str() {
                "--headless" => {
                    bare!("--headless");
                    parsed.headless = true;
                }
                "--spectator" => {
                    bare!("--spectator");
                    parsed.spectator = true;
                }
                "--bots" => {
                    parsed.num_bots = number!("--bots");
                    botted = Some(position);
                }
                "--ticks" => parsed.ticks = Some(Ticks(number!("--ticks"))),
                "--level" => open!("--level", Load::Level(value!("--level"))),
                "--load" => open!("--load", Load::Save(SaveSlot(number!("--load")))),
                "--demo" => open!("--demo", Load::Demo(PathBuf::from(value!("--demo")))),
                "--record" => parsed.record = Some(PathBuf::from(value!("--record"))),
                "--state" => parsed.state = Some(PathBuf::from(value!("--state"))),
                "--seat" => parsed.seat = PlayerId(number!("--seat")),
                "--listen" => parsed.listen = Some(number!("--listen")),
                "--connect" => {
                    parsed.connect = Some(value!("--connect"));
                    peered = Some(position);
                }
                // `-h` alongside the long spelling, because it is what
                // somebody types first and a runtime that answered
                // `-h is not an argument this runtime has` for it would be
                // making a point rather than helping. It is the only short
                // flag: the rest take values, and a one-letter flag with a
                // value is where a hand-written parser starts guessing.
                "--help" | "-h" => return Err(Argument::Help),
                _ => return Err(Argument::Unknown { argument: flag }),
            }
        }

        parsed.coherent(botted, peered)?;
        Ok(parsed)
    }

    /// The refusals that are about a whole command line rather than about one
    /// flag, checked once everything has been read.
    ///
    /// They are here rather than in the loop above because a flag that is
    /// refused for what another flag says cannot be judged until that other flag
    /// has had its chance to appear, in either order. `botted` and `peered` are
    /// where `--bots` and `--connect` were written, so a message about the pair
    /// names them in the order the operator typed them.
    ///
    /// Neither is gated on the `net` feature. What they judge is the command
    /// line, which says the same thing whether or not this build has a socket to
    /// act on it with -- and a build that quietly accepted a link it cannot open
    /// would be the surprise this is here to avoid.
    fn coherent(&self, botted: Option<usize>, peered: Option<usize>) -> Result<(), Argument> {
        // Two flags name one link, and either of them alone names half of one.
        // Both directions are refused, because both are a command line that
        // asked for another machine and would have got a run playing alone:
        // `--listen` with nobody to reach binds a port and waits for a datagram
        // that is never sent, and `--connect` with no socket to send from has
        // nowhere for the answer to arrive. Neither is a run somebody typed
        // three words to ask for, and a runtime that quietly played it alone
        // would be answering a different question.
        //
        // Before the bots check below, so that an operator is told about the
        // link that is not a link before being told what cannot go beside it.
        match (self.listen, self.connect.as_deref()) {
            (Some(_), None) => Err(Argument::Incomplete {
                flag: "--listen",
                needs: "--connect",
            }),
            (None, Some(_)) => Err(Argument::Incomplete {
                flag: "--connect",
                needs: "--listen",
            }),
            // A bot is a controller, and the bot is asked only on the path a run
            // with nobody else in it takes -- a linked run never asks it at all.
            // So a run given both would have accepted a number of bots and
            // played none of those seats, which is a flag that did nothing.
            // `App::run` refuses the pair as well, for the run a harness builds
            // by hand; this is the half an operator is told about before
            // anything opens.
            //
            // Well founded because of the arms above: `--connect` is never
            // written without `--listen`, so naming it names the whole link
            // rather than the half of it that happened to be looked at.
            //
            // `--bots 0` is not the half of anything: it asks for no bots, which
            // is what a run without the flag has.
            (Some(_), Some(_)) => match (self.num_bots, botted, peered) {
                (1.., Some(bots), Some(connect)) if bots < connect => Err(Argument::Conflicting {
                    flags: ["--bots", "--connect"],
                }),
                (1.., Some(_), Some(_)) => Err(Argument::Conflicting {
                    flags: ["--connect", "--bots"],
                }),
                _ => Ok(()),
            },
            (None, None) => Ok(()),
        }
    }
}
