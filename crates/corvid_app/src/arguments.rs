//! The command line every Corvid game answers to, parsed by hand.

use std::{fmt, path::PathBuf};

use corvid_behavior::SaveSlot;

use crate::Retention;

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
/// | `--saves DIR` | put the save slots under `DIR` rather than under `./saves/NAME/` |
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
/// a zero status, because asking for the usage is not a failure — and it does
/// so through [`io::Write`](std::io::Write) on a handle rather than through a
/// printing macro. [`parse`](Arguments::parse) reports [`Argument::Help`]
/// instead, whose [`Display`](fmt::Display) is this text, which is what a
/// harness driving a run by hand sees.
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
  --saves DIR       put the save slots under DIR rather than under saves/NAME
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
