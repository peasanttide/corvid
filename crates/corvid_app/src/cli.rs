//! The command line: what a Corvid game answers to, how it is read, and the one
//! `main` that acts on it.
//!
//! Parsing, the usage text and every byte this crate writes to the process's
//! streams live here together, so that "what the command line does" is one file
//! rather than a parser in one and a printer in another. The three writes below
//! — the usage and the digest on stdout, a refused command line on stderr — are
//! the only ones in the crate, they are `println!` and `eprintln!` under named
//! exceptions rather than handles that dodge the lint, and they are all in
//! sight of each other.

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
    /// A level reference, as the JSON of
    /// [`LevelRef`](corvid_replay::LevelRef) — the type a game's own
    /// [`Level::Reference`](corvid_behavior::Level::Reference) is.
    ///
    /// Kept as text rather than parsed here, because what it parses *into* is
    /// the game's own type and this parser knows no game. [`App::run`] is what
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
/// | `--listen PORT` \| `--connect HOST:PORT` | the socket the other machine is behind |
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
    pub listen: Option<u16>,
    /// Where the other machine is, as `HOST:PORT`.
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
        [--seat N] [--listen PORT] [--connect HOST:PORT]

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
  --listen PORT     bind this UDP port
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
    /// [`Argument::NotANumber`] for a count that is not one, and
    /// [`Argument::Conflicting`] for two flags that cannot both be acted on.
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

        // A bot is a controller, and a controller is no part of what a session
        // records — so a run that filled a seat locally and had a peer in the
        // same session would be writing a column every other machine writes
        // differently. `App::run` refuses the pair as well, for the run a
        // harness builds by hand; this is the half an operator is told about
        // before anything opens.
        //
        // `--bots 0` is not the half of anything: it asks for no bots, which is
        // what a run without the flag has.
        if parsed.num_bots > 0
            && let (Some(bots), Some(connect)) = (botted, peered)
        {
            return Err(Argument::Conflicting {
                flags: if bots < connect {
                    ["--bots", "--connect"]
                } else {
                    ["--connect", "--bots"]
                },
            });
        }

        Ok(parsed)
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
    /// [`parse`](Arguments::parse), because a build without the `net` feature
    /// has no socket to open and the same three flags are then three settings
    /// that do nothing rather than a contradiction.
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
            Self::NotALevel { value, why } => {
                write!(
                    f,
                    "--level was given {value}, which is not a level this game has: {why}\n\n\
                     {USAGE}"
                )
            }
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
/// # use corvid_files::{Malformed, Source};
/// # use corvid_replay::{Opening, Opens, Profile, Schema, Seed};
/// # use corvid_time::Tick;
/// # use serde::{Deserialize, Serialize};
/// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// # struct Nowhere;
/// # impl Level for Nowhere {
/// #     type Reference = String;
/// #     fn load(_: &String, _: &dyn Source) -> Result<Self, Malformed> { Ok(Self) }
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
/// fn main() -> corvid_app::Result {
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
/// it to **stdout** and answers `Ok(())`, so the process exits zero and a shell
/// script does not have to special-case it. [`Arguments::parse`] still reports
/// it as [`Argument::Help`], because that function may not print.
///
/// A command line that could not be acted on writes the reason and the usage to
/// **stderr** and stops the process with status 2 — rather than handing back an
/// `Err`, which the runtime would print with [`Debug`] and collapse to status 1.
/// So neither kind of answer arrives here as an error, and this function's `Err`
/// is a run that started and could not finish.
///
/// This module is the only one in the crate that writes to the process's
/// streams at all, and it does so through `println!` and `eprintln!` under
/// stated exceptions rather than through handles that would reach the same
/// streams while passing the lint. A harness that wants none of that drives
/// [`App::launch`], which reads the same command line and hands
/// [`Error::Argument`](crate::Error::Argument) back without writing anything.
///
/// # What it does with the exit code
///
/// A [`quit`](corvid_behavior::Command::quit) names a status, and a status that
/// is not [`SUCCESS`](corvid_behavior::ExitCode::SUCCESS) leaves the process
/// with that number rather than with the `1` that any `Err` from a `main`
/// collapses to. Everything a run writes down has been written by then: a
/// recording is closed and a save is on disk before the run hands its outcome
/// back.
///
/// # Errors
///
/// Whatever [`App::run`] reports. A command line that could not be acted on is
/// not among them: it is written to stderr and the process stops with status 2
/// before a run is built.
pub fn main<G: Game>() -> crate::Result
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
        return Ok(());
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
            Err(why) => return Err(why),
        },
        // A `--listen` with nobody to reach, or a `--connect` with no socket to
        // reach it from, is half a link: the run plays alone, which is what it
        // would have done with neither.
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
        Err(why) => return Err(why),
    };
    finish::<G>(&outcome, headless);
    Ok(())
}

/// What a process that could not read its command line exits with.
///
/// Two, which is the conventional status for a usage error and is deliberately
/// not one: every `Err` a `main` hands back collapses to one, so a run that
/// failed for a reason that is not the command line stays distinguishable from
/// a command line nobody could act on.
const REFUSED: i32 = 2;

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
/// rather than the 1 an `Err` collapses to.
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
        reason = "the third and last write to the process's streams in this crate, and it is here for the reason the other two are: a program that was handed a command line it cannot act on says so where an operator reads it. stderr rather than stdout, because a program's own answer belongs alone on stdout for a pipe"
    )]
    {
        eprintln!("{why}");
    }
    std::process::exit(REFUSED);
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
