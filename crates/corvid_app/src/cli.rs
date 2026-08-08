//! The command line: what a Corvid game answers to, how it is read, and the one
//! `main` that acts on it.
//!
//! Parsing, the usage text and every byte this crate writes to the process's
//! streams live here together, so that "what the command line does" is one file
//! rather than a parser in one and a printer in another. The two writes to
//! stdout are the only ones in the crate, they are `println!` under a named
//! exception rather than handles that dodge the lint, and they are both in
//! sight of each other.

use std::{fmt, path::PathBuf};

use corvid_behavior::SaveSlot;
use corvid_hash::digest;
use corvid_replay::Opens;

use crate::{App, Error, Outcome, Retention};
// `crate::Result` is spelled in full at each use below rather than imported:
// this file also parses into `Result<_, Argument>`, and one `Result` in scope
// standing for a one-parameter alias would shadow the other.

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
/// | `--ticks N` | stop once `N` ticks have run, counted from where the run opened |
/// | `--capture DIR` | write the run down under `DIR` |
/// | `--retain N` \| `--retain all` | keep at least `N` ticks of the session, or all of it |
/// | `--replay FILE` | open on the session recorded in `FILE` |
/// | `--load N` | open on save slot `N` |
/// | `--saves DIR` | put the save slots under `DIR` rather than under `$XDG_DATA_HOME/NAME/saves/` |
/// | `--help`, `-h` | write this table to stdout and stop, successfully |
///
/// Every one of them is a thing the *operator* decides rather than the game:
/// whether this machine has a display, how long to run for, whether to record
/// it, how much to keep, and which recorded run to open on. A setting only the
/// game can know — its opening, its rules, its passes — is not here and should
/// not be, because a flag for it would be a flag whose legal values only the
/// game could list.
///
/// `--ticks 100` and `--ticks=100` are the same argument. A flag that takes a
/// value and is given none is [`Argument::Missing`] rather than a default,
/// because a run of "0 ticks" and a run of "as long as you like" are both things
/// somebody might have meant.
///
/// # Why no argument-parsing library
///
/// Seven flags, no subcommands, no completion, and a workspace habit of owning
/// small things. The whole of the parser below is shorter than the manifest
/// entry and the feature audit a dependency would cost, and it is what the
/// crate's public error type has to describe anyway.
///
/// ```
/// use corvid_app::{Arguments, Retention};
///
/// let arguments = Arguments::parse(["--headless", "--ticks=90"])?;
/// assert!(arguments.headless);
/// assert_eq!(arguments.ticks, Some(90));
/// assert_eq!(arguments.capture, None);
///
/// // The two spellings are the same argument, and `all` is the word for
/// // keeping the whole session.
/// let recorded = Arguments::parse(["--capture", "out/", "--retain", "all"])?;
/// assert_eq!(recorded.retain, Some(Retention::Everything));
///
/// // A slot is a number, and where to look for it is a path.
/// let resumed = Arguments::parse(["--load", "3", "--saves", "slots/"])?;
/// assert_eq!(resumed.load.map(|slot| slot.0), Some(3));
/// assert_eq!(resumed.saves.as_deref(), Some(std::path::Path::new("slots/")));
/// # Ok::<(), corvid_app::Argument>(())
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Arguments {
    /// Whether the run was told to open nothing.
    pub headless: bool,
    /// How many ticks to run for, if a number was given.
    pub ticks: Option<u64>,
    /// Where to write the run down, if anywhere.
    pub capture: Option<PathBuf>,
    /// How much of the session to keep, if the operator said.
    pub retain: Option<Retention>,
    /// The recorded session to open on, if one was named.
    pub replay: Option<PathBuf>,
    /// The save slot to open on, if one was named.
    pub load: Option<SaveSlot>,
    /// Where the save slots live, if the operator said.
    pub saves: Option<PathBuf>,
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
corvid: [--headless] [--ticks N] [--capture DIR] [--retain N|all]
        [--replay FILE] [--load N] [--saves DIR]

  --headless        play with no window, no adapter and no audio device
  --ticks N         stop once N ticks have run, counted from where the run
                    opened
  --capture DIR     write the run down under DIR: one audio frame per displayed
                    frame and a picture of it where there is an adapter to draw
                    one, plus the hash trace and the session
  --retain N|all    keep at least N ticks of the session in memory, or all of
                    it; a capture keeps all of it unless this says otherwise
  --replay FILE     open on the session recorded in FILE, which is the session
                    file a --capture wrote, and carry it on
  --load N          open on save slot N rather than on the game's own opening
  --saves DIR       put the save slots under DIR rather than the user data dir
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
    /// [`Argument::Unexpected`] for a value on a flag that takes none, and
    /// [`Argument::NotANumber`] for a count that is not one.
    pub fn parse<I, S>(arguments: I) -> Result<Self, Argument>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut parsed = Self::default();
        let mut arguments = arguments.into_iter().map(Into::into);

        while let Some(argument) = arguments.next() {
            // `--flag=value` and `--flag value` are the same argument. The split
            // is on the first `=` so that a path with one in it survives being
            // passed as `--capture=a=b`.
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

            match flag.as_str() {
                "--headless" => {
                    bare!("--headless");
                    parsed.headless = true;
                }
                "--ticks" => {
                    let value = value!("--ticks");
                    parsed.ticks = Some(value.parse().map_err(|_| Argument::NotANumber {
                        flag: "--ticks",
                        value,
                    })?);
                }
                "--capture" => parsed.capture = Some(PathBuf::from(value!("--capture"))),
                "--replay" => parsed.replay = Some(PathBuf::from(value!("--replay"))),
                "--saves" => parsed.saves = Some(PathBuf::from(value!("--saves"))),
                "--load" => {
                    let value = value!("--load");
                    parsed.load = Some(SaveSlot(value.parse().map_err(|_| {
                        Argument::NotANumber {
                            flag: "--load",
                            value,
                        }
                    })?));
                }
                "--retain" => {
                    let value = value!("--retain");
                    parsed.retain = Some(if value == "all" {
                        Retention::Everything
                    } else {
                        Retention::Recent {
                            ticks: value.parse().map_err(|_| Argument::NotANumber {
                                flag: "--retain",
                                value,
                            })?,
                        }
                    });
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
        }
    }
}

impl std::error::Error for Argument {}

/// How big a run with a device and no window draws.
///
/// A run like that has nowhere to show a frame, so the only thing the size
/// decides is what a `--capture` writes. Seven hundred and twenty rows is what
/// a picture is expected to be when nobody said; a run that wants another size
/// builds its own [`App`] and calls
/// [`offscreen`](App::offscreen).
#[cfg(not(feature = "window"))]
#[cfg(feature = "render")]
const OFFSCREEN: corvid_render::Extent = corvid_render::Extent::new(1280, 720);

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
/// // A game that draws nothing says nothing at all: `App`'s renderer defaults
/// // to `()`, which opens no device and is never asked to draw. There is no
/// // line to write, which is the shortest way of drawing nothing there is.
/// fn main() -> corvid_app::Result {
///     corvid_app::main::<Server>()
/// }
/// ```
///
/// A window, a headless run, a capture, a replay and a save slot are all the
/// same program: this reads the process's arguments and decides. **A game never
/// asks for determinism**, because a game that had to call `.headless()` would
/// have a mode that is deterministic and a mode that is not, and only one of
/// them would be tested. [`Arguments`] is the list of what an operator may say.
///
/// # Which backend this picks
///
/// `--headless` is no window, no adapter and no audio device. Without it, a
/// build with the `window` feature opens a window; a build without one draws
/// into a texture if there is a `--capture` to write the pictures into, and
/// plays headless otherwise, because a device drawing frames nothing will look
/// at or keep is a device doing nothing. Every build can open a device, so
/// `--headless` means the same thing in all of them — which is what lets a
/// script pass it without knowing which build it is talking to.
///
/// # The bound, and the game that has nothing to draw
///
/// `S: State`, which is `G: Render`: the client-local half is one chain of
/// traits over one marker, so a game that reaches this has a `setup` and a
/// `draw` whether it draws anything or not. There is no configuration in which
/// the bound is weaker, and there used to be — a trait reconciling `Render`
/// with `Present` under opposite `cfg`s, which existed because a `wgpu` type
/// could not be named a crate lower down. It can now.
///
/// `type Graphics = ();` is the one line that satisfies the bound for a game
/// with nothing to draw: a dedicated server, a determinism check, or a game
/// that has not drawn anything yet writes that line and writes no `wgpu`. The
/// view is declared on [`Render`](corvid_render::Render) rather than beside
/// it, because the macro supplies the whole implementation and only the game
/// knows what its view is.
///
/// # `--help` is not a failure
///
/// An operator who asked for the usage got what they asked for. This writes it
/// to **stdout** and answers `Ok(())`, so the process exits zero and a shell
/// script does not have to special-case it. [`Arguments::parse`] still reports
/// it as [`Argument::Help`], because that function may not print. This module is
/// the only one in the crate that writes to the process's streams at all, and it
/// does so through `println!` under a stated exception rather than through a
/// handle that would reach the same stream while passing the lint. Every other
/// command line that could not be acted on goes to stderr through the `Err` this
/// hands back, and exits non-zero.
///
/// # What it does with the exit code
///
/// A [`quit`](corvid_behavior::Command::quit) names a status, and a status that
/// is not [`SUCCESS`](corvid_behavior::ExitCode::SUCCESS) leaves the process
/// with that number rather than with the `1` that any `Err` from a `main`
/// collapses to. Everything a run writes down has been written by then: a
/// capture is closed and a save is on disk before the run hands its outcome
/// back.
///
/// # Errors
///
/// [`Error::Argument`] for a command line that could not be acted on, and then
/// whatever [`App::run`] reports.
pub fn main<S: corvid_behavior::State + Opens>() -> crate::Result {
    let Some(arguments) = command_line()? else {
        return Ok(());
    };
    // Whether the operator asked for a run with no devices, which is the one
    // thing that decides whether the ending tick and digest go to stdout.
    let headless = arguments.headless;
    // The declaration, and then the table written against it. Neither of these
    // was here once, and a game played through this `main` therefore ran with
    // an empty declaration — which binds no key and no axis and answers
    // `RELEASED` to every query for the length of the run. `Present::SETS` is
    // what closed it, and it is required rather than defaulted so that the
    // same hole cannot be dug again.
    let app = App::<S>::new()
        .opening(S::opening())
        .input(corvid_input::Input::new(&[]));
    // The table is a windowed run's business alone: `App::bindings` holds a
    // `corvid_window::Bindings` and a run with no window reads no device.
    #[cfg(feature = "window")]
    let app = app;
    let app = if arguments.headless {
        app
    } else {
        #[cfg(feature = "window")]
        {
            app.window()
        }
        // No window, and a capture asked for: draw into a texture instead, so
        // that a build machine still gets a picture. A build with no graphics
        // stack at all has no third case — it writes the audio, the trace and
        // the session, and says nothing about pixels.
        #[cfg(all(not(feature = "window"), feature = "render"))]
        if arguments.capture.is_some() {
            app.offscreen(OFFSCREEN)
        } else {
            app
        }
        #[cfg(all(not(feature = "window"), not(feature = "render")))]
        {
            app
        }
    };
    finish(&app.arguments(arguments).run()?, headless);
    Ok(())
}

/// Reads the process's arguments, answering a request for the usage on the way.
///
/// [`None`] is "the usage was asked for and has been written, and there is no
/// game left to play" — which is a success, and is why this is an `Option`
/// inside the `Ok` rather than a third error variant.
///
/// # Errors
///
/// [`Error::Argument`] for a command line that could not be acted on.
fn command_line() -> crate::Result<Option<Arguments>> {
    match Arguments::from_env() {
        Ok(arguments) => Ok(Some(arguments)),
        Err(Argument::Help) => {
            #[allow(
                clippy::print_stdout,
                reason = "the same exception `finish` carries, and the other half of it: an operator who passed `--help` asked for this text on stdout. Writing it to an `io::stdout()` handle instead passed the lint while doing the identical thing"
            )]
            {
                println!("{}", Arguments::USAGE);
            }
            Ok(None)
        }
        Err(why) => Err(Error::Argument(why)),
    }
}

/// Reports where the run got to, and leaves the process with the status a
/// [`quit`](corvid_behavior::Command::quit) named.
///
/// The report is a `tracing` event, so a game that wants it structured installs
/// a subscriber. A **headless** run also writes the ending tick and digest to
/// stdout, because that is what an operator asked for by passing `--headless`
/// and a `main` of three lines has nowhere to install a subscriber. A windowed
/// run prints nothing: the digest is not what somebody watching a window came
/// for.
///
/// The stdout line is a `println!` under a named exception rather than a write
/// to a handle. Both reach the same stream; only one of them says so where a
/// reader looking for the workspace's printing rule will find it.
fn finish<S: corvid_behavior::State>(outcome: &Outcome<S>, headless: bool) {
    if headless {
        #[allow(
            clippy::print_stdout,
            reason = "this crate's `main` is a program rather than a library: an operator who passed `--headless` asked for this line on stdout, and a `main` of three lines has nowhere to install a subscriber. Writing to an `io::stdout()` handle instead would pass the lint while doing the identical thing, which is worse — the exception belongs where a reader can see it"
        )]
        {
            println!(
                "tick {} mark {}",
                outcome.session.last(),
                digest(&outcome.state)
            );
        }
    }
    tracing::info!(
        name: "corvid_app.finished",
        tick = %outcome.session.last(),
        mark = %digest(&outcome.state),
        requests = outcome.requests.len(),
        "the run ended",
    );
    if outcome.exit != corvid_behavior::ExitCode::SUCCESS {
        // The run is over and everything it writes down is written: `App::run`
        // closes a capture before it hands an outcome back. What is left to do
        // is hand the operating system the number the game asked for, and a
        // `main` returning `Result` cannot — every `Err` from one is status 1.
        std::process::exit(i32::from(outcome.exit.0));
    }
}
