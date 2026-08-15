//! What a command answers with, and what the registry shows about it.
//!
//! The seam against `console.rs` is that nothing here runs anything: these are
//! the values that cross in and out of a registry, and they are the whole of
//! what a surface drawing a console has to understand.

use crate::{Invalid, Parameter};

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
