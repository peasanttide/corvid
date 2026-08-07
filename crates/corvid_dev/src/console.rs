//! The registry: typed commands, and the help and completion they generate.

use core::fmt;

use crate::{Argument, Arguments, Invalid, Parameter};

/// What a command answers with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Reply {
    /// Nothing to say. The usual case.
    #[default]
    Done,
    /// One line for the log.
    Said(String),
    /// It could not.
    Refused(Invalid),
}

impl Reply {
    /// One line for the log.
    #[must_use]
    #[inline]
    pub fn said(line: impl Into<String>) -> Self {
        Self::Said(line.into())
    }

    /// Whether the command refused.
    #[must_use]
    #[inline]
    pub const fn is_refused(&self) -> bool {
        matches!(self, Self::Refused(_))
    }

    /// Why it refused, if it did.
    #[must_use]
    #[inline]
    pub const fn refusal(&self) -> Option<&Invalid> {
        match self {
            Self::Refused(invalid) => Some(invalid),
            Self::Done | Self::Said(_) => None,
        }
    }
}

impl From<Invalid> for Reply {
    #[inline]
    fn from(invalid: Invalid) -> Self {
        Self::Refused(invalid)
    }
}

impl From<String> for Reply {
    #[inline]
    fn from(line: String) -> Self {
        Self::Said(line)
    }
}

/// One registered command, as the widget that lists them reads it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    /// What the line names it.
    pub path: &'static str,
    /// One line about what it does.
    pub help: &'static str,
    /// What it takes, generated from the types of its parameters.
    pub parameters: Vec<Parameter>,
}

impl Entry {
    /// The usage line: the path, then every parameter.
    #[must_use]
    pub fn usage(&self) -> String {
        let mut usage = self.path.to_owned();
        for parameter in &self.parameters {
            usage.push(' ');
            usage.push_str(&parameter.to_string());
        }
        usage
    }
}

/// One thing completion offers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Completion {
    /// What to put on the line.
    pub text: String,
    /// What to show beside it: a command's help, or a parameter's type.
    pub help: &'static str,
}

/// One line of generated help.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HelpLine {
    /// The command.
    pub path: &'static str,
    /// [`Entry::usage`].
    pub usage: String,
    /// [`Entry::help`].
    pub help: &'static str,
}

/// What a closure has to be to be a command.
///
/// Implemented for every `FnMut` of up to four [`Argument`]s returning a
/// [`Reply`], which is what lets a command be written as the closure its
/// signature already describes rather than as a parser plus a doc comment.
pub trait Handler<A: Arguments>: Send + 'static {
    /// Run it.
    fn call(&mut self, arguments: A) -> Reply;
}

impl<F: FnMut() -> Reply + Send + 'static> Handler<()> for F {
    fn call(&mut self, (): ()) -> Reply {
        self()
    }
}

macro_rules! handler {
    ($($letter:ident $value:ident),+) => {
        impl<F, $($letter: Argument),+> Handler<($($letter,)+)> for F
        where
            F: FnMut($($letter),+) -> Reply + Send + 'static,
        {
            fn call(&mut self, ($($value,)+): ($($letter,)+)) -> Reply {
                self($($value),+)
            }
        }
    };
}

handler!(A a);
handler!(A a, B b);
handler!(A a, B b, C c);
handler!(A a, B b, C c, D d);

/// One command, erased down to the words it is given and the names its
/// parameters are shown under.
type Run = Box<dyn FnMut(&[&str], &[Parameter]) -> Reply + Send>;

/// The registry.
///
/// Commands are kept sorted by path, so [`help`](Self::help),
/// [`complete`](Self::complete) and [`entries`](Self::entries) are stable
/// whatever order a game registered them in.
///
/// ```
/// use corvid_dev::{Console, Invalid, Reply};
///
/// let mut console = Console::new();
/// console
///     .register("wave.skip", "advance to the next wave", |to: Option<u32>| {
///         Reply::said(format!("skipping to {to:?}"))
///     })
///     .named(&["to"]);
///
/// // The usage line is read off the parameter's type, not written down.
/// assert_eq!(console.help(Some("wave.skip"))[0].usage, "wave.skip [to: u32]");
///
/// assert_eq!(console.run("wave.skip 3"), Reply::said("skipping to Some(3)"));
/// assert_eq!(console.run("wave.skip"), Reply::said("skipping to None"));
///
/// // And a word of the wrong type is refused by name.
/// assert_eq!(
///     console.run("wave.skip soon").refusal(),
///     Some(&Invalid::Malformed {
///         parameter: "to",
///         of: "u32",
///         given: "soon".to_owned(),
///     }),
/// );
/// ```
#[derive(Default)]
pub struct Console {
    entries: Vec<Entry>,
    runs: Vec<Run>,
}

impl Console {
    /// An empty registry.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            runs: Vec::new(),
        }
    }

    /// Register a command.
    ///
    /// The parameter types produce the help and the completion, so neither is
    /// written down. Registering a path twice replaces what was there.
    ///
    /// Rust does not expose a closure's parameter names, so the parameters
    /// arrive numbered and [`Registered::named`] is how a command that wants
    /// its help to read `[to: u32]` says so.
    pub fn register<A, H>(
        &mut self,
        path: &'static str,
        help: &'static str,
        mut run: H,
    ) -> Registered<'_>
    where
        A: Arguments + 'static,
        H: Handler<A>,
    {
        let entry = Entry {
            path,
            help,
            parameters: A::PARAMETERS.to_vec(),
        };
        let run: Run = Box::new(
            move |words, parameters| match A::parse_as(words, parameters) {
                Ok(arguments) => run.call(arguments),
                Err(invalid) => Reply::Refused(invalid),
            },
        );
        let at = match self.entries.binary_search_by(|entry| entry.path.cmp(path)) {
            Ok(at) => {
                if let Some(slot) = self.entries.get_mut(at) {
                    *slot = entry;
                }
                if let Some(slot) = self.runs.get_mut(at) {
                    *slot = run;
                }
                at
            }
            Err(at) => {
                self.entries.insert(at, entry);
                self.runs.insert(at, run);
                at
            }
        };
        Registered { console: self, at }
    }

    /// Run a line.
    ///
    /// An empty line does nothing. An unknown command and a bad argument each
    /// answer with a [`Reply::Refused`] rather than panicking.
    pub fn run(&mut self, line: &str) -> Reply {
        let mut words = line.split_whitespace();
        let Some(path) = words.next() else {
            return Reply::Done;
        };
        let rest: Vec<&str> = words.collect();
        let unknown = || {
            Reply::Refused(Invalid::Unknown {
                path: path.to_owned(),
            })
        };
        let Ok(at) = self.entries.binary_search_by(|entry| entry.path.cmp(path)) else {
            return unknown();
        };
        // The names the help shows are the names a refusal has to use, so the
        // entry's parameters go in beside the words.
        let (Some(entry), Some(run)) = (self.entries.get(at), self.runs.get_mut(at)) else {
            return unknown();
        };
        run(&rest, &entry.parameters)
    }

    /// What completes `prefix`, in the order a list should show them.
    ///
    /// A prefix with no command yet completes command paths; one naming a
    /// command completes that command's next parameter from its
    /// [`Argument::CANDIDATES`].
    ///
    /// ```
    /// use corvid_dev::{Console, Reply};
    ///
    /// let mut console = Console::new();
    /// console.register("wave.skip", "advance a wave", |_to: Option<u32>| Reply::Done);
    /// console.register("wave.stop", "stop the wave", || Reply::Done);
    /// console.register("time.pause", "pause", |on: bool| Reply::said(on.to_string()));
    ///
    /// let under = console.complete("wave.");
    /// assert_eq!(under.len(), 2);
    /// assert_eq!(under[0].text, "wave.skip");
    /// assert_eq!(under[0].help, "advance a wave");
    ///
    /// // A `bool` parameter offers what a `bool` can be.
    /// let values = console.complete("time.pause t");
    /// assert_eq!(values.len(), 1);
    /// assert_eq!(values[0].text, "true");
    /// ```
    #[must_use]
    pub fn complete(&self, prefix: &str) -> Vec<Completion> {
        let trailing = prefix.ends_with(char::is_whitespace);
        let mut words = prefix.split_whitespace();
        let Some(path) = words.next() else {
            return self.entries.iter().map(Completion::command).collect();
        };
        let rest: Vec<&str> = words.collect();
        if rest.is_empty() && !trailing {
            return self
                .entries
                .iter()
                .filter(|entry| entry.path.starts_with(path))
                .map(Completion::command)
                .collect();
        }
        let Ok(at) = self.entries.binary_search_by(|entry| entry.path.cmp(path)) else {
            return Vec::new();
        };
        let Some(entry) = self.entries.get(at) else {
            return Vec::new();
        };
        let (index, word) = if trailing {
            (rest.len(), "")
        } else {
            (
                rest.len().saturating_sub(1),
                rest.last().copied().unwrap_or_default(),
            )
        };
        let Some(parameter) = entry.parameters.get(index) else {
            return Vec::new();
        };
        parameter
            .candidates
            .iter()
            .filter(|candidate| candidate.starts_with(word))
            .map(|candidate| Completion {
                text: (*candidate).to_owned(),
                help: parameter.of,
            })
            .collect()
    }

    /// The generated help for one command, or for every command.
    ///
    /// Sorted by path either way, so the list is stable.
    #[must_use]
    pub fn help(&self, path: Option<&str>) -> Vec<HelpLine> {
        self.entries
            .iter()
            .filter(|entry| path.is_none_or(|path| entry.path == path))
            .map(|entry| HelpLine {
                path: entry.path,
                usage: entry.usage(),
                help: entry.help,
            })
            .collect()
    }

    /// Everything registered, for the widget that lists them.
    #[must_use]
    #[inline]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// How many commands are registered.
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is registered.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Completion {
    fn command(entry: &Entry) -> Self {
        Self {
            text: entry.path.to_owned(),
            help: entry.help,
        }
    }
}

/// Prints what is registered, which is everything about a console that can be
/// printed: a command is a closure.
impl fmt::Debug for Console {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Console")
            .field("entries", &self.entries)
            .finish_non_exhaustive()
    }
}

/// The command that was just registered, so its parameters can be named.
#[derive(Debug)]
pub struct Registered<'a> {
    console: &'a mut Console,
    at: usize,
}

impl Registered<'_> {
    /// Give the parameters the names the help should show, in order.
    ///
    /// Names past the end of the parameter list are ignored, and parameters
    /// past the end of `names` keep their numbers.
    pub fn named(&mut self, names: &[&'static str]) {
        if let Some(entry) = self.console.entries.get_mut(self.at) {
            for (parameter, name) in entry.parameters.iter_mut().zip(names) {
                parameter.name = name;
            }
        }
    }

    /// The command as it now reads.
    #[must_use]
    pub fn entry(&self) -> Option<&Entry> {
        self.console.entries.get(self.at)
    }
}
