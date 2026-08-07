//! What a level is read through, and the two ways reading one fails.

use alloc::{string::String, vec::Vec};

/// A path that is not there.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Missing {
    /// What was asked for.
    pub path: String,
}

impl Missing {
    /// A path that is not there.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

impl core::fmt::Display for Missing {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "nothing to read at {}", self.path)
    }
}

impl core::error::Error for Missing {}

/// Bytes that are there and are not what they claim to be.
///
/// The other half of [`Missing`], and worth separating from it: data that is
/// absent is a deployment short a file, and data that is present and will not
/// parse is a build that disagrees with its data. Only one of those is fixed by
/// copying something.
///
/// # Why the path is optional
///
/// Two callers raise this and they know different amounts.
/// [`Level::load`](../corvid_behavior/trait.Level.html#tymethod.load) was handed
/// a reference and reads its own files, so it knows which one objected;
/// `corvid_asset`'s `Asset::decode` is handed bytes and nothing else, because
/// the store that read them is what knows where they came from. One type with
/// [`at`](Self::at) and [`new`](Self::new) rather than two types with the same
/// name, which is what this workspace already has too many of.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Malformed {
    /// What was read, for a caller that knew.
    pub path: Option<String>,
    /// What was wrong with it, in the decoder's own words.
    pub why: String,
}

impl Malformed {
    /// What the decoder objected to, by a caller that does not know the path.
    #[must_use]
    pub fn new(why: impl Into<String>) -> Self {
        Self {
            path: None,
            why: why.into(),
        }
    }

    /// The same, by a caller that does.
    #[must_use]
    pub fn at(path: impl Into<String>, why: impl Into<String>) -> Self {
        Self {
            path: Some(path.into()),
            why: why.into(),
        }
    }

    /// Names the path on a failure raised by something that did not know it.
    ///
    /// This is what a store does with what a decoder handed back: the decoder
    /// said what was wrong and the store says which file it was wrong in.
    #[must_use]
    pub fn in_file(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }
}

impl core::fmt::Display for Malformed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match &self.path {
            Some(path) => write!(f, "{path} could not be read: {}", self.why),
            None => write!(f, "malformed asset: {}", self.why),
        }
    }
}

impl core::error::Error for Malformed {}

impl From<&str> for Malformed {
    fn from(why: &str) -> Self {
        Self::new(why)
    }
}

impl From<String> for Malformed {
    fn from(why: String) -> Self {
        Self::new(why)
    }
}

/// The one crossing between the two failures, and it goes this way only.
///
/// A load that could not find its file has failed to load, so a `Missing`
/// widens into the error `Level::load` answers. Nothing narrows the other way:
/// data that would not parse is not data that was absent, and a caller that
/// retried the second would retry the first for ever.
impl From<Missing> for Malformed {
    fn from(missing: Missing) -> Self {
        Self::at(missing.path, "it is not there")
    }
}

/// Somewhere a level's files come from: a directory, an archive, a map in
/// memory, or whatever a platform has instead of any of those.
///
/// `Send + Sync` because loading runs on a loader thread and the source is
/// shared with it.
///
/// # It is synchronous, and that is a decision
///
/// A level is read off the tick, on a thread of its own, so blocking costs a
/// simulation nothing — and the barrier that keeps two peers applying a level
/// at the same tick is built on a load being a thing that finishes rather than
/// a thing that is polled. A platform with only asynchronous reads is a later
/// problem, and this trait is where it will be solved rather than a reason to
/// make every game's `load` async today.
pub trait Source: Send + Sync {
    /// The file's bytes.
    ///
    /// # Errors
    ///
    /// [`Missing`] for a path this source has nothing under, which includes a
    /// path it refuses to follow.
    fn read(&self, path: &str) -> Result<Vec<u8>, Missing>;

    /// Every path beginning with `prefix`, in order.
    ///
    /// **A string prefix, not a directory.** `list("level/court")` matches
    /// `level/courtyard.bin` as well as `level/court.bin`, which is what a
    /// caller asking for a prefix asked for.
    ///
    /// **Ordered, and that is load-bearing.** A level built out of whatever
    /// this answered would otherwise have contents that depend on the
    /// implementation's iteration order, and a peer that walked its props in a
    /// different order is a peer that hashes a different level.
    ///
    /// A prefix with nothing under it answers an empty list rather than an
    /// error: "there are no props in this level" is an answer, and a level that
    /// has to name every file it might read is a level that cannot grow one.
    /// What *is* an error is a source that could not be asked at all — a
    /// directory whose permissions refuse to say what is in it.
    ///
    /// # Errors
    ///
    /// [`Missing`] for a prefix this source cannot be asked about: one it
    /// refuses to follow, or one whose directory will not say what is in it.
    /// **Not** for a prefix with nothing under it, which answers an empty list.
    fn list(&self, prefix: &str) -> Result<Vec<String>, Missing>;

    /// Whether it is there, without wanting its bytes.
    ///
    /// The default reads the file and throws the bytes away, which is correct
    /// and is not what any real source should do. It is a default rather than a
    /// requirement so that a source implementing this trait owes two methods
    /// instead of three; both of the ones shipped here override it, and so
    /// should a directory, an archive or anything else that can answer the
    /// question by asking about a name rather than about a file.
    fn exists(&self, path: &str) -> bool {
        self.read(path).is_ok()
    }
}

/// Every `Source` is one behind a reference, so a `&dyn Source` can be handed on
/// without a second indirection.
///
/// `Level::load` takes `&dyn Source`, and a caller holding a `&Files` would
/// otherwise have to name the coercion at every site.
impl<T: Source + ?Sized> Source for &T {
    fn read(&self, path: &str) -> Result<Vec<u8>, Missing> {
        (**self).read(path)
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, Missing> {
        (**self).list(prefix)
    }

    fn exists(&self, path: &str) -> bool {
        (**self).exists(path)
    }
}

/// Nothing at all, for a game whose levels are constants.
///
/// A `Level::load` that ignores its source — pong's court is written in the
/// source and read from no file — still has to be handed one, and this is what
/// a caller with nothing to hand over hands over. Every read fails; that is the
/// honest answer for a source with no files in it, and a `load` that ignores it
/// never asks.
impl Source for () {
    fn read(&self, path: &str) -> Result<Vec<u8>, Missing> {
        Err(Missing::new(path))
    }

    fn list(&self, _prefix: &str) -> Result<Vec<String>, Missing> {
        Ok(Vec::new())
    }

    fn exists(&self, _path: &str) -> bool {
        false
    }
}
