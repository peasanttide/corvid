//! What goes wrong, and why each of them is worth its own name.

use alloc::string::{String, ToString};
use core::fmt;

/// A value could not be written down, or bytes could not be read back.
///
/// The cases are separated because they mean different things to whoever is
/// holding the bytes. [`Wrote`](Self::Wrote) is a bug in the type and is the
/// same every time. [`Read`](Self::Read) is usually a capture that does not
/// match the types reading it, which is what a version skew looks like from the
/// receiving end. [`TooLarge`](Self::TooLarge) is the one that can arrive from a
/// hostile peer rather than a mismatched one. [`Trailing`](Self::Trailing) is
/// the one worth being loud about: the bytes decoded, and there were more of
/// them than the value needed, so what was recorded is *not* what was just read.
///
/// A type that needs its field names alongside its values is split across the
/// first two, and which half it lands in is worth knowing before reading a
/// failure. `#[serde(flatten)]` serializes as a map whose length is not known
/// in advance and never reaches a decoder at all:
/// `Err(Wrote("Serde(SequenceMustHaveLength)"))`. An untagged enum writes down
/// without complaint — its *reader* is the half that wants the names — and
/// fails on the way back with `Err(Read("Serde(AnyNotSupported)"))`. Both are
/// findings rather than false alarms, and `tests/named.rs` pins them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// The value could not be written down at all, with the encoder's reason.
    ///
    /// A `Serialize` that refuses, or one that asks for something this format
    /// cannot write: a sequence or a map that will not say how long it is,
    /// which is the shape `#[serde(flatten)]` has.
    Wrote(String),
    /// The bytes could not be read back, with the decoder's reason. See
    /// [`decode`](crate::decode) for which shapes land here.
    Read(String),
    /// More bytes than a capture may hold, which is
    /// [`CEILING`](crate::CEILING).
    ///
    /// Writing knows how many, because it has them. Reading knows only that the
    /// bytes asked for more than the ceiling — the number is refused on sight,
    /// before anything is allocated on the strength of it, which is the whole
    /// reason the ceiling exists.
    TooLarge {
        /// How many bytes the value came to, when that is known.
        wrote: Option<usize>,
    },
    /// The value was read and the bytes were longer than it.
    Trailing {
        /// How many bytes the value consumed.
        used: usize,
        /// How many bytes were offered.
        len: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wrote(why) => write!(f, "the value could not be written down: {why}"),
            Self::Read(why) => write!(f, "the bytes could not be read back: {why}"),
            Self::TooLarge { wrote: Some(len) } => write!(
                f,
                "the value wrote down as {len} bytes, and a capture may hold {}",
                crate::CEILING,
            ),
            Self::TooLarge { wrote: None } => write!(
                f,
                "the bytes ask to allocate more than the {} a capture may hold, \
                 so they were refused before anything was read",
                crate::CEILING,
            ),
            Self::Trailing { used, len } => write!(
                f,
                "the value read in {used} of {len} bytes and {} were left over, \
                 so these bytes are not the value that was recorded",
                // Saturating, because this variant's fields are `pub` on an
                // enum whose `#[non_exhaustive]` stops exhaustive matching and
                // not construction: a caller can build one with `used > len`,
                // and a `Display` that panicked on it would abort on the one
                // path that exists to explain a failure.
                len.saturating_sub(*used),
            ),
        }
    }
}

impl core::error::Error for Error {}

impl Error {
    /// The encoder's own error, kept as text.
    ///
    /// The message is carried rather than the error, so that no caller can match
    /// on which encoder produced it and come to depend on that encoder — which
    /// is the whole failure this crate exists to prevent, arriving through the
    /// error type instead of through a manifest.
    pub(crate) fn wrote(why: &impl fmt::Display) -> Self {
        Self::Wrote(why.to_string())
    }

    /// The decoder's own error, kept as text, for the same reason.
    pub(crate) fn read(why: &impl fmt::Display) -> Self {
        Self::Read(why.to_string())
    }
}
