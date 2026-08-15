//! What a command line could not be read as.
//!
//! The seam against `arguments.rs` is that nothing here parses anything: these
//! are the refusals, and each one writes the sentence an operator reads.

#[cfg(feature = "net")]
use corvid_behavior::PlayerId;

use crate::cli::arguments::USAGE;

/// An argument this runtime could not act on.
///
/// [`Help`](Self::Help) is the odd one and is deliberate: asking for the usage
/// is not a failure, and it arrives here because the parser that noticed it may
/// not print. [`main`](crate::main) is what turns it back into a success -- the
/// usage on stdout and a zero status -- and a harness driving a run by hand
/// matches on it and does whatever it likes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum Argument {
    /// The usage was asked for. Its [`Display`](core::fmt::Display) is the usage.
    #[error("{}", USAGE)]
    Help,
    /// A flag this runtime does not have.
    #[error("{argument} is not an argument this runtime has\n\n{}", USAGE)]
    Unknown {
        /// What was passed.
        argument: String,
    },
    /// A flag that takes a value, with nothing after it.
    #[error("{flag} takes a value and was given none\n\n{}", USAGE)]
    Missing {
        /// Which flag.
        flag: &'static str,
    },
    /// A value on a flag that takes none.
    #[error("{flag} takes no value\n\n{}", USAGE)]
    Unexpected {
        /// Which flag.
        flag: &'static str,
    },
    /// A count that is not a number.
    #[error("{flag} takes a number and was given {value}\n\n{}", USAGE)]
    NotANumber {
        /// Which flag.
        flag: &'static str,
        /// What was passed for it.
        value: String,
    },
    /// Two flags that cannot both be acted on.
    #[error("{} and {} cannot both be given\n\n{}", flags[0], flags[1], USAGE)]
    Conflicting {
        /// Which two, in the order they were written.
        flags: [&'static str; 2],
    },
    /// A flag that means nothing without another one, written without it.
    ///
    /// [`Conflicting`](Self::Conflicting) is the other half of the same idea and
    /// says the opposite thing, which is why it is not this: two flags that
    /// cannot both be given, against two that have to be given together.
    #[error("{flag} means nothing without {needs}\n\n{}", USAGE)]
    Incomplete {
        /// What was written.
        flag: &'static str,
        /// What it needs beside it.
        needs: &'static str,
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
    /// [`parse`](crate::Arguments::parse), and gated on the `net` feature for the same
    /// reason: a build without it has no socket to open, so the three flags are
    /// three settings that do nothing rather than a contradiction, and a variant
    /// nothing in such a build can construct would be a variant a caller has to
    /// match on and never see.
    #[cfg(feature = "net")]
    #[error(
        "this machine plays seat {}, and --listen with --connect can only arrange the pair of \
         seats 0 and 1: a session with more machines in it is assembled by a lobby, which is \
         told who sits where rather than computing it\n\n{}",
        seat.0,
        USAGE
    )]
    Pairing {
        /// The seat that was asked for.
        seat: PlayerId,
    },
    /// A `--level` naming a level this game's loader would not build.
    ///
    /// What refused is [`Level::load`](corvid_behavior::Level::load) itself: a
    /// game whose levels are self-describing never sees this, and a game that
    /// reads its levels from somewhere a command line cannot reach always does
    /// -- which is the honest answer for a flag that has no way to be told
    /// where those files are.
    ///
    /// The reason it carries a [`String`] rather than the game's own error:
    /// this type is [`PartialEq`] and [`Hash`](core::hash::Hash), which
    /// [`Level::Error`](corvid_behavior::Level::Error) is not asked to be, and
    /// what a reader wants out of it is the sentence anyway.
    #[error(
        "--level was given {value}, and this game will not build that level from its name \
         alone: {why}\n\n{}",
        USAGE
    )]
    UnreadableLevel {
        /// What was passed.
        value: String,
        /// What the game's own loader said.
        why: String,
    },
}
