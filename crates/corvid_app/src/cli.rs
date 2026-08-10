//! The command line: what a Corvid game answers to, how it is read, and the one
//! `main` that acts on it.
//!
//! Parsing, the usage text and every byte this crate writes to the process's
//! streams live here together, so that "what the command line does" is one file
//! rather than a parser in one and a printer in another. The four writes below
//! — the usage and the digest on stdout, a refused command line and a run that
//! could not finish on stderr — are the only ones in the crate, they are
//! `println!` and `eprintln!` under named exceptions rather than handles that
//! dodge the lint, and they are all in sight of each other.

use std::{fmt, path::PathBuf};

use corvid_behavior::{PlayerId, SaveSlot};
use corvid_control::Controller;
use corvid_replay::Opens;
use corvid_time::{Tick, Ticks};

use crate::{
    App, Outcome,
    game::{AuralizerConfig, BotConfig, ControllerConfig, Game, RenderConfig},
};
// `crate::Result` is spelled in full at each use below rather than imported:
// this file also parses into `Result<_, Argument>`, and one `Result` in scope
// standing for a one-parameter alias would shadow the other.

/// What the run opens on.
///
/// Three ways of naming a starting point and one field to hold whichever was
/// given, because they are one decision: a run opens on a level, on a slot or
/// on a recording, and a command line that named two of them named two runs.
/// [`Argument::Conflicting`] is what that is.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Load {
    /// A level's name, as the JSON of the string
    /// [`Level::load`](corvid_behavior::Level::load) reads.
    ///
    /// Kept as JSON rather than taken bare, because a level name is a value in
    /// a save file as well as a word on a command line, and quoting it once
    /// means the two spell it the same way. [`App::run`] is what
    /// reads it, and a string that is not that game's level reference is
    /// [`Argument::NotALevel`] there.
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
/// | `--level JSON` | open on this level rather than on the game's own |
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
/// setting only the game can know — its rules, its passes, its opening — is not
/// here and should not be, because a flag for it would be a flag whose legal
/// values only the game could list. `--level` is the near miss and the reason
/// it works: what it carries is the game's own reference type, as JSON, so this
/// parser holds a string and the game is what reads it.
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
/// [`main`](crate::main) is the one place that writes it — to stdout, and with
/// a zero status, because asking for the usage is not a failure.
/// [`parse`](Arguments::parse) reports [`Argument::Help`] instead, whose
/// [`Display`](fmt::Display) is this text, which is what a harness driving a run
/// by hand sees. That split is the reason this text is public at all: a parser
/// that may not print and a `main` that may are two different jobs, and only one
/// of them is allowed the stream.
const USAGE: &str = "\
corvid: [--headless] [--spectator] [--bots N] [--ticks N]
        [--level JSON | --load N | --demo FILE] [--record FILE] [--state DIR]
        [--seat N] [--listen PORT --connect HOST:PORT]

  --headless        play with no window, no adapter and no audio device
  --spectator       claim no seat: submit nothing, and watch the seat this
                    machine would have played
  --bots N          let the game's bot play N seats nobody is in
  --ticks N         stop once N ticks have run, counted from where the run
                    opened
  --level JSON      open on this level rather than the game's own
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
    /// thing this does that [`parse`](Self::parse) does not — and the reason
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
    /// reported as one because this function may not print — [`main`](crate::main)
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
    /// act on it with — and a build that quietly accepted a link it cannot open
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
            // with nobody else in it takes — a linked run never asks it at all.
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

/// An argument this runtime could not act on.
///
/// [`Help`](Self::Help) is the odd one and is deliberate: asking for the usage
/// is not a failure, and it arrives here because the parser that noticed it may
/// not print. [`main`](crate::main) is what turns it back into a success — the
/// usage on stdout and a zero status — and a harness driving a run by hand
/// matches on it and does whatever it likes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Argument {
    /// The usage was asked for. Its [`Display`](fmt::Display) is the usage.
    Help,
    /// A flag this runtime does not have.
    Unknown {
        /// What was passed.
        argument: String,
    },
    /// A flag that takes a value, with nothing after it.
    Missing {
        /// Which flag.
        flag: &'static str,
    },
    /// A value on a flag that takes none.
    Unexpected {
        /// Which flag.
        flag: &'static str,
    },
    /// A count that is not a number.
    NotANumber {
        /// Which flag.
        flag: &'static str,
        /// What was passed for it.
        value: String,
    },
    /// Two flags that cannot both be acted on.
    Conflicting {
        /// Which two, in the order they were written.
        flags: [&'static str; 2],
    },
    /// A flag that means nothing without another one, written without it.
    ///
    /// [`Conflicting`](Self::Conflicting) is the other half of the same idea and
    /// says the opposite thing, which is why it is not this: two flags that
    /// cannot both be given, against two that have to be given together.
    Incomplete {
        /// What was written.
        flag: &'static str,
        /// What it needs beside it.
        needs: &'static str,
    },
    /// A `--level` that is not JSON this game's level reference deserializes
    /// from.
    ///
    /// The reason it carries a [`String`] rather than the error that produced
    /// it: this type is [`PartialEq`], which a `serde_json::Error` is not, and
    /// what a reader wants out of it is the sentence anyway.
    NotALevel {
        /// What was passed.
        value: String,
        /// Why it could not be read.
        why: String,
    },
    /// A `--listen`/`--connect` from a seat that pair of flags cannot arrange.
    ///
    /// Two flags name one other machine, so what they can express is the pair
    /// of seats zero and one: this machine announces itself as its own seat and
    /// reaches the other of the two. A seat above one has no third address to
    /// connect to and no peer number to announce that anybody is expecting, and
    /// computing one would open a link that carries datagrams and matches no
    /// seat at the far end. A session with more machines in it is assembled by
    /// a lobby, which is told who sits where.
    ///
    /// Noticed when the socket is opened rather than by
    /// [`parse`](Arguments::parse), and gated on the `net` feature for the same
    /// reason: a build without it has no socket to open, so the three flags are
    /// three settings that do nothing rather than a contradiction, and a variant
    /// nothing in such a build can construct would be a variant a caller has to
    /// match on and never see.
    #[cfg(feature = "net")]
    Pairing {
        /// The seat that was asked for.
        seat: PlayerId,
    },
    /// A `--level` that names one of this game's levels and could not be
    /// loaded without the game's files.
    ///
    /// The reference was read; what refused is
    /// [`Level::load`](corvid_behavior::Level::load), handed the empty source.
    /// A game whose levels are self-describing never sees this, and a game that
    /// reads its levels from files always does — which is the honest answer for
    /// a flag that has no way to be told where those files are.
    UnreadableLevel {
        /// What was passed.
        value: String,
        /// What the game's own loader said.
        why: String,
    },
}

impl fmt::Display for Argument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Help => f.write_str(USAGE),
            Self::Unknown { argument } => {
                write!(
                    f,
                    "{argument} is not an argument this runtime has\n\n{USAGE}"
                )
            }
            Self::Missing { flag } => {
                write!(f, "{flag} takes a value and was given none\n\n{USAGE}")
            }
            Self::Unexpected { flag } => {
                write!(f, "{flag} takes no value\n\n{USAGE}")
            }
            Self::NotANumber { flag, value } => {
                write!(f, "{flag} takes a number and was given {value}\n\n{USAGE}")
            }
            Self::Conflicting { flags } => {
                let [first, second] = flags;
                write!(f, "{first} and {second} cannot both be given\n\n{USAGE}")
            }
            Self::Incomplete { flag, needs } => {
                write!(f, "{flag} means nothing without {needs}\n\n{USAGE}")
            }
            Self::NotALevel { value, why } => {
                write!(
                    f,
                    "--level was given {value}, which is not a level this game has: {why}\n\n\
                     {USAGE}"
                )
            }
            #[cfg(feature = "net")]
            Self::Pairing { seat } => {
                write!(
                    f,
                    "this machine plays seat {}, and --listen with --connect can only arrange \
                     the pair of seats 0 and 1: a session with more machines in it is assembled \
                     by a lobby, which is told who sits where rather than computing it\n\n{USAGE}",
                    seat.0
                )
            }
            Self::UnreadableLevel { value, why } => {
                write!(
                    f,
                    "--level was given {value}, and this game reads that level from files \
                     that a command line has no way to hand it: {why}\n\n{USAGE}"
                )
            }
        }
    }
}

impl std::error::Error for Argument {}

/// Installs the subscriber that makes this framework's own events visible.
///
/// Every crate here reports through `tracing` — which adapter was chosen, which
/// frames were dropped, what the netcode did with a late datagram — and **not
/// one of them appears without a subscriber installed**.
///
/// It is still a binary's decision, which is why this is a function a `main`
/// calls rather than something a library does on its own: a library that
/// installed a subscriber would be a library nobody can silence. What it stops
/// is every game writing the same twelve lines to make the framework audible.
/// [`main`] calls it; a game building its own [`App`] calls this or does not.
///
/// `RUST_LOG` picks the level, as it does everywhere else; the default is
/// `info`, which is the level a chosen adapter and a dropped frame are reported
/// at. `RUST_LOG=corvid_net=debug` is how a link's individual datagrams become
/// visible.
///
/// Events go to **stderr**, which leaves a program's own answer alone on stdout
/// for a pipe.
///
/// Calling it twice is not an error and not this function's business: a
/// subscriber already installed stays, and a game that installed its own before
/// reaching a Corvid `main` keeps it.
pub fn watch() {
    use tracing_subscriber::{EnvFilter, fmt};

    drop(
        fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .with_writer(std::io::stderr)
            .with_target(true)
            .try_init(),
    );
}

/// Plays the game the process was started to play.
///
/// **This is the whole of a Corvid `main`, and there is no second spelling.**
///
/// ```no_run
/// # use std::sync::Arc;
/// # use corvid_behavior::{Level, State};
/// # use corvid_files::{};
/// # use corvid_replay::{Opening, Opens, Profile, Schema, Seed};
/// # use corvid_time::Tick;
/// # use serde::{Deserialize, Serialize};
/// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// # struct Nowhere;
/// # impl Level for Nowhere {
/// #     type Error = core::convert::Infallible;
/// #     fn load(_: &str) -> Result<Self, core::convert::Infallible> { Ok(Self) }
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
/// [`Controller::SETS`] and the binding table is
/// [`Controller::bindings`] — the three things only the game can state.
/// Everything else on the [`App`] comes from the command line, through
/// [`App::arguments`], so an operator's flag beats a default whichever order
/// they were written in.
///
/// # Which backend this picks
///
/// `--headless` is no window, no adapter and no audio device. Without it, a
/// build with the `window` feature opens a window and a build without one plays
/// headless, because a device drawing frames nothing will look at is a device
/// doing nothing. Every build can play headless, so `--headless` means the same
/// thing in all of them — which is what lets a script pass it without knowing
/// which build it is talking to. A harness that wants an adapter and no window
/// builds its own [`App`] and calls
/// [`offscreen`](App::offscreen).
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
/// reads, and each is the error's own [`Display`](fmt::Display) rather than the
/// [`Debug`] that a `main` returning `Result` would have printed — the
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
/// out of a `main` collapses to anyway — so keeping it for the run means a
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
        // parser — so it gets the answer every other refused command line gets,
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
/// One, which is what any `Err` handed back from a `main` collapses to — so a
/// script that reads a status rather than a stream sees the number it expects
/// for a program that failed, and the sentence saying why is on stderr for a
/// reader who wants it.
const FAILED: i32 = 1;

/// Reads the process's arguments, and answers whatever they asked for that is
/// not a game.
///
/// [`None`] is "the usage was asked for and has been written, and there is no
/// game left to play" — a success, and the process exits zero.
///
/// A command line that could **not** be acted on is written to stderr and the
/// process stops with [`REFUSED`], rather than travelling back as an `Err`.
/// Both halves of that are the point. The message is
/// [`Argument`]'s [`Display`](fmt::Display) — the sentence and the usage under
/// it — where an `Err` out of a `main` is printed by the runtime with
/// [`Debug`], which would show an operator `Argument(Conflicting { flags:
/// [...] })` and no list of what the runtime accepts. And the status is 2
/// rather than the [`FAILED`] a run that broke leaves.
///
/// It is the same shape the `--help` arm has, one stream over: this crate's
/// `main` is a program, and a program answers a command line it was given.
/// [`App::launch`] is the library half — it hands [`Error::Argument`] back and
/// writes nothing — which is what a harness driving a run by hand wants.
fn command_line() -> Option<Arguments> {
    match Arguments::from_env() {
        Ok(arguments) => Some(arguments),
        Err(Argument::Help) => {
            #[allow(
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
/// The one place either kind of refusal is answered — the parser's, and the
/// `--level` that only the game could judge — so an operator gets the same
/// sentence, on the same stream, with the same status, wherever it was noticed.
///
/// Nothing is open when this is reached: no window, no adapter, no capture
/// directory, no file. So there is nothing here that unwinding would close and
/// exiting will not.
fn refuse(why: &Argument) -> ! {
    #[allow(
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
/// [`Error`](crate::Error) writes a sentence for every one of its variants —
/// which file could not be written and why, which port would not bind, which
/// tick two peers disagreed at — and **not one of those sentences reaches an
/// operator through a `main` that hands the error back**, because the runtime
/// prints a returned `Err` with [`Debug`]. So this prints the
/// [`Display`](fmt::Display), for the same reason and on the same stream
/// [`refuse`] does, one status down.
///
/// Nothing this process opened is still open when this is reached, so exiting
/// closes nothing that unwinding would have: [`App::run`] takes its app by
/// value, and a window, an adapter, a recording and a capture directory are all
/// dropped inside it before the `Err` comes back.
fn failed(why: &crate::Error) -> ! {
    #[allow(
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
/// session — which is prediction working rather than anything disagreeing. A
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
/// a subscriber — [`watch`] is what a `main` calls to get one. A **headless**
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
        #[allow(
            clippy::print_stdout,
            reason = "this crate's `main` is a program rather than a library: an operator who passed `--headless` asked for this line on stdout, and a `main` of one line has nowhere to install a subscriber. Writing to an `io::stdout()` handle instead would pass the lint while doing the identical thing, which is worse — the exception belongs where a reader can see it"
        )]
        {
            println!("{mark}");
        }
    }

    if outcome.exit != corvid_behavior::ExitCode::SUCCESS {
        // The run is over and everything it writes down is written: `App::run`
        // closes a recording before it hands an outcome back. What is left to
        // do is hand the operating system the number the game asked for, and a
        // `main` returning `Result` cannot — every `Err` from one is status 1.
        std::process::exit(i32::from(outcome.exit.0));
    }
}
