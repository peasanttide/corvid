//! What a command takes, and the help that is generated from it.

use core::fmt;

use corvid_fixed::I16F16;
use corvid_time::Tick;
/// One parameter, as the help and the completion read it.
///
/// Every field comes from the type of a command's parameter, so a command whose
/// signature changes has its help change with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Parameter {
    /// What the help calls it.
    pub name: &'static str,
    /// What the help calls its type.
    pub of: &'static str,
    /// Whether omitting it is legal.
    pub required: bool,
    /// Whether it takes every word that is left.
    pub repeated: bool,
    /// What completion offers for it. Empty for an open-ended type.
    pub candidates: &'static [&'static str],
}

impl Parameter {
    /// The parameter that one [`Argument`] type is, under `name`.
    ///
    /// ```
    /// use corvid_dev::Parameter;
    ///
    /// let to = Parameter::of::<Option<u32>>("to");
    /// assert_eq!(to.of, "u32");
    /// assert!(!to.required);
    /// assert_eq!(to.to_string(), "[to: u32]");
    /// ```
    #[must_use]
    pub const fn of<A: Argument>(name: &'static str) -> Self {
        Self {
            name,
            of: A::TYPE,
            required: A::REQUIRED,
            repeated: A::REPEATED,
            candidates: A::CANDIDATES,
        }
    }
}

/// `<name: type>` when it is required and `[name: type]` when it is not, with a
/// trailing `...` on one that takes every word left.
impl fmt::Display for Parameter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (open, close) = if self.required {
            ('<', '>')
        } else {
            ('[', ']')
        };
        let more = if self.repeated { "..." } else { "" };
        write!(f, "{open}{}: {}{more}{close}", self.name, self.of)
    }
}

/// Why a line was not run.
#[derive(Clone, Debug, PartialEq, Eq, Hash, thiserror::Error)]
pub enum Invalid {
    /// Nothing is registered under that path. Nothing is suggested either: a
    /// console that runs the wrong command is worse than one that runs none.
    #[error("no command called {path}")]
    Unknown {
        /// The path as the line spelled it.
        path: String,
    },
    /// A required parameter was left off.
    #[error("{parameter} is required, and is a {of}")]
    Missing {
        /// The parameter's name.
        parameter: &'static str,
        /// The type it wanted.
        of: &'static str,
    },
    /// Words past the last parameter.
    #[error("{words} words too many")]
    Extra {
        /// How many, saturating at 255.
        words: u8,
    },
    /// A word that is not a value of the type its parameter names.
    #[error("{parameter} is a {of}, and {given} is not one")]
    Malformed {
        /// The parameter's name.
        parameter: &'static str,
        /// The type it wanted.
        of: &'static str,
        /// The word that was given instead.
        given: String,
    },
    /// A tunable was asked for a value its range does not allow.
    #[error("{path} takes {low} to {high}, and not {given}")]
    OutOfRange {
        /// The tunable's path.
        path: &'static str,
        /// The lowest value it takes.
        low: I16F16,
        /// The highest.
        high: I16F16,
        /// What was asked for.
        given: I16F16,
    },
}

/// A value a console command may take.
///
/// Implemented here for the primitives, for [`String`], for [`Tick`], for
/// [`I16F16`], and for [`Option<T>`] and [`Vec<T>`] over any of them. A command
/// wanting something else implements it for that type, which is the friction
/// that keeps a console command from becoming an API:
///
/// ```
/// use corvid_dev::Argument;
///
/// #[derive(Debug, PartialEq)]
/// enum Speed { Slow, Fast }
///
/// impl Argument for Speed {
///     const TYPE: &'static str = "speed";
///     const CANDIDATES: &'static [&'static str] = &["slow", "fast"];
///
///     fn parse(text: &str) -> Option<Self> {
///         match text {
///             "slow" => Some(Self::Slow),
///             "fast" => Some(Self::Fast),
///             _ => None,
///         }
///     }
/// }
///
/// assert_eq!(Speed::parse("fast"), Some(Speed::Fast));
/// assert_eq!(Speed::parse("brisk"), None);
/// ```
pub trait Argument: Sized {
    /// What the help calls this type.
    const TYPE: &'static str;

    /// Whether omitting it is legal. False for [`Option<T>`] and [`Vec<T>`],
    /// which is where the square brackets in a usage line come from.
    ///
    /// It must be `false` exactly when [`take`](Self::take) answers for an
    /// empty slice, and `tests/console.rs` checks the pair for every type
    /// implemented here.
    const REQUIRED: bool = true;

    /// Whether it takes every word that is left. True for [`Vec<T>`] alone,
    /// which is therefore only useful as a command's last parameter.
    const REPEATED: bool = false;

    /// What completion offers. Empty for an open-ended type.
    const CANDIDATES: &'static [&'static str] = &[];

    /// One word into a value.
    ///
    /// [`None`] rather than an error: the only thing an implementation could
    /// say is that it could not, and the caller is the one that knows which
    /// parameter this was and can therefore build the [`Invalid`] that names
    /// it.
    fn parse(text: &str) -> Option<Self>;

    /// Take this parameter's value off the front of the words that are left,
    /// answering it and how many words it used.
    ///
    /// The default takes exactly one word, which is what every scalar type
    /// does. [`Option<T>`] takes none when there are none and [`Vec<T>`] takes
    /// all of them.
    #[must_use]
    fn take(words: &[&str]) -> Option<(Self, usize)> {
        Self::parse(words.first()?).map(|value| (value, 1))
    }
}

macro_rules! scalar {
    ($($type:ty => $name:literal),+ $(,)?) => {
        $(
            impl Argument for $type {
                const TYPE: &'static str = $name;

                fn parse(text: &str) -> Option<Self> {
                    text.parse().ok()
                }
            }
        )+
    };
}

scalar! {
    u8 => "u8",
    u16 => "u16",
    u32 => "u32",
    u64 => "u64",
    usize => "usize",
    i8 => "i8",
    i16 => "i16",
    i32 => "i32",
    i64 => "i64",
    isize => "isize",
    char => "char",
    String => "text",
}

impl Argument for bool {
    const TYPE: &'static str = "bool";
    const CANDIDATES: &'static [&'static str] = &["false", "true"];

    fn parse(text: &str) -> Option<Self> {
        text.parse().ok()
    }
}

/// The tick a slider scrubs to, spelled as the number a trace prints.
impl Argument for Tick {
    const TYPE: &'static str = "tick";

    fn parse(text: &str) -> Option<Self> {
        text.parse().ok().map(Tick)
    }
}

/// The workspace's fixed point, through the decimal a person types.
///
/// A value outside what `I16F16` holds is refused rather than saturated: a
/// tunable that silently clamped would be a `Rules` change nobody agreed to.
impl Argument for I16F16 {
    const TYPE: &'static str = "number";

    fn parse(text: &str) -> Option<Self> {
        text.parse().ok().and_then(Self::checked_from_f64)
    }
}

/// A parameter a line may leave off.
impl<T: Argument> Argument for Option<T> {
    const TYPE: &'static str = T::TYPE;
    const REQUIRED: bool = false;
    const CANDIDATES: &'static [&'static str] = T::CANDIDATES;

    fn parse(text: &str) -> Option<Self> {
        T::parse(text).map(Some)
    }

    fn take(words: &[&str]) -> Option<(Self, usize)> {
        words.first().map_or_else(
            || Some((None, 0)),
            |word| T::parse(word).map(|value| (Some(value), 1)),
        )
    }
}

/// Every word that is left, which is why it belongs last.
impl<T: Argument> Argument for Vec<T> {
    const TYPE: &'static str = T::TYPE;
    const REQUIRED: bool = false;
    const REPEATED: bool = true;
    const CANDIDATES: &'static [&'static str] = T::CANDIDATES;

    fn parse(text: &str) -> Option<Self> {
        T::parse(text).map(|value| vec![value])
    }

    fn take(words: &[&str]) -> Option<(Self, usize)> {
        words
            .iter()
            .map(|word| T::parse(word))
            .collect::<Option<Self>>()
            .map(|values| (values, words.len()))
    }
}

/// A command's whole parameter list.
///
/// Implemented for the empty tuple and for tuples of up to four
/// [`Argument`]s. [`Console::register`](crate::Console::register) reads
/// [`PARAMETERS`](Self::PARAMETERS) to build the help and the completion, so
/// neither is written down.
pub trait Arguments: Sized {
    /// The list, in the order the command declared them.
    const PARAMETERS: &'static [Parameter];

    /// The words of a line, after the command's path, into the whole list.
    ///
    /// # Errors
    ///
    /// [`Invalid::Missing`] for a required parameter with no word left,
    /// [`Invalid::Malformed`] for a word that is not a value of its parameter's
    /// type, and [`Invalid::Extra`] for words past the last parameter.
    fn parse(words: &[&str]) -> Result<Self, Invalid> {
        Self::parse_as(words, Self::PARAMETERS)
    }

    /// The same, against the names a console gave the parameters.
    ///
    /// A refusal names the parameter, and the name the console shows is the one
    /// the refusal has to use -- otherwise a line refused by `wave.skip [to:
    /// u32]` would complain about a parameter called `1`. Positions `parameters`
    /// is too short for fall back to [`PARAMETERS`](Self::PARAMETERS).
    ///
    /// # Errors
    ///
    /// The same three as [`parse`](Self::parse).
    fn parse_as(words: &[&str], parameters: &[Parameter]) -> Result<Self, Invalid>;
}

/// One parameter, or the [`Invalid`] that names it.
fn take<A: Argument>(words: &[&str], parameter: Parameter) -> Result<(A, usize), Invalid> {
    if let Some(taken) = A::take(words) {
        return Ok(taken);
    }
    Err(words.first().map_or_else(
        || Invalid::Missing {
            parameter: parameter.name,
            of: parameter.of,
        },
        |given| Invalid::Malformed {
            parameter: parameter.name,
            of: parameter.of,
            given: (*given).to_owned(),
        },
    ))
}

/// Words past the last parameter.
fn extra(rest: &[&str]) -> Result<(), Invalid> {
    if rest.is_empty() {
        Ok(())
    } else {
        Err(Invalid::Extra {
            words: u8::try_from(rest.len()).unwrap_or(u8::MAX),
        })
    }
}

impl Arguments for () {
    const PARAMETERS: &'static [Parameter] = &[];

    fn parse_as(words: &[&str], _parameters: &[Parameter]) -> Result<Self, Invalid> {
        extra(words)
    }
}

macro_rules! tuple {
    ($($letter:ident $value:ident $slot:literal),+) => {
        impl<$($letter: Argument),+> Arguments for ($($letter,)+) {
            const PARAMETERS: &'static [Parameter] = &[
                $(Parameter::of::<$letter>($slot),)+
            ];

            fn parse_as(words: &[&str], parameters: &[Parameter]) -> Result<Self, Invalid> {
                let mut rest = words;
                let mut at = 0_usize;
                $(
                    let named = parameters
                        .get(at)
                        .copied()
                        .unwrap_or(Parameter::of::<$letter>($slot));
                    let ($value, used) = take::<$letter>(rest, named)?;
                    rest = rest.get(used..).unwrap_or(&[]);
                    at = at.saturating_add(1);
                )+
                let _ = at;
                extra(rest)?;
                Ok(($($value,)+))
            }
        }
    };
}

tuple!(A a "1");
tuple!(A a "1", B b "2");
tuple!(A a "1", B b "2", C c "3");
tuple!(A a "1", B b "2", C c "3", D d "4");
