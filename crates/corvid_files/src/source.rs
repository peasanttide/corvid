//! What a level is read through, the two ways reading one fails, and the one
//! way writing to it does.

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
/// Two callers raise this and they know different amounts. `Level::load` over
/// in `corvid_behavior` was handed a reference and reads its own files, so it
/// knows which one objected; a decoder handed bytes and nothing else cannot,
/// because whatever read them is what knows where they came from. One type with
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

/// A write that did not happen.
///
/// The counterpart to [`Missing`], and folded the same way: one type for every
/// reason a source did not take the bytes, because a caller that has been told
/// its write did not land has been told the thing it can act on. A source that
/// refuses every write and a source that refused this one -- a path outside what
/// it will follow, a device with nothing left on it -- raise the same finding,
/// exactly as a permission and an absent file both raise a `Missing`.
///
/// The name is for the common case, which is that the source is a directory
/// nobody mounted for writing or one of the two read-only sources shipped here.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReadOnly {
    /// What the write was aimed at.
    pub path: String,
}

impl ReadOnly {
    /// A write that did not happen.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

impl core::fmt::Display for ReadOnly {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "nothing can be written to {}", self.path)
    }
}

impl core::error::Error for ReadOnly {}

/// Somewhere a level's files come from: a directory, an archive, a map in
/// memory, or whatever a platform has instead of any of those.
///
/// `Send + Sync` because loading runs on a loader thread and the source is
/// shared with it.
///
/// # It is synchronous, and that is a decision
///
/// A level is read off the tick, on a thread of its own, so blocking costs a
/// simulation nothing -- and the barrier that keeps two peers applying a level
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
    /// path it refuses to follow and a file it can see and cannot open -- a
    /// permission it does not have, an archive member that will not inflate.
    /// There is one read failure and it means the bytes did not arrive, the
    /// same way [`list`](Self::list) folds a directory that will not say what
    /// is in it into the same type. [`Malformed`] is not raised here at all:
    /// bytes that arrived and will not parse are a finding of whoever parses
    /// them, and this method's job ends once they are in hand.
    fn read(&self, path: &str) -> Result<Vec<u8>, Missing>;

    /// Every path in this source, in order.
    ///
    /// **Ordered, and that is load-bearing.** A level built out of whatever
    /// this answered would otherwise have contents that depend on the
    /// implementation's iteration order, and a peer that walked its props in a
    /// different order is a peer that hashes a different level.
    ///
    /// **Everything, and the caller narrows it.** A source is the files one
    /// level is read through rather than a whole disk, so the listing is the
    /// whole of it and a caller wanting a subset writes the `filter` it wants.
    /// That keeps the one rule a source owes -- sorted -- from having to hold
    /// alongside a matching rule about what counts as being under a name, which
    /// is the part every implementation would have spelled differently.
    ///
    /// A source with nothing in it answers an empty list rather than an error:
    /// "there are no props in this level" is an answer, and a level that has to
    /// name every file it might read is a level that cannot grow one. What *is*
    /// an error is a source that could not be asked at all -- a directory whose
    /// permissions refuse to say what is in it.
    ///
    /// # Errors
    ///
    /// [`Missing`] for a source that cannot be asked, naming whatever the
    /// source calls the place it could not enumerate. **Not** for a source with
    /// nothing in it, which answers an empty list.
    fn list(&self) -> Result<Vec<String>, Missing>;

    /// Puts bytes at a path, replacing whatever was there.
    ///
    /// Defaulted to refusing, because reading is what this trait is for and
    /// most of what implements it is a directory mounted for reading, an
    /// archive, or a constant compiled into the binary. A source that can take
    /// bytes overrides this; one that cannot owes nothing.
    ///
    /// The refusal is a returned [`ReadOnly`] rather than a second trait or a
    /// `can_write` a caller is expected to consult first. A capability asked
    /// about separately is a capability that can change between the question
    /// and the write, and the answer the caller needs is the same either way:
    /// the bytes are not there.
    ///
    /// **`&mut self`, which is what keeps a load from writing.** `Level::load`
    /// is handed a `&dyn Source`, and a shared reference cannot reach this
    /// method at all -- so a level that tried to write during its own load
    /// fails to compile rather than at run time. The blanket impl on `&T`
    /// inherits this default for the same reason.
    ///
    /// # Errors
    ///
    /// [`ReadOnly`] when the bytes did not land, whether because this source
    /// takes no writes at all or because it refused this one.
    fn write(&mut self, path: &str, _bytes: &[u8]) -> Result<(), ReadOnly> {
        Err(ReadOnly::new(path))
    }

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

/// A borrow of a `Source` is a `Source`, so code generic over one takes either.
///
/// This buys nothing at a `&dyn Source` parameter -- `&Memory` already coerces
/// to `&dyn Source` on its own, and `Level::load` needs no help. What it buys
/// is the bound `S: Source`: a caller that only borrows its source, or that was
/// handed the `&dyn Source` from a `load` further up, can satisfy that bound
/// without owning, cloning or re-boxing anything. `?Sized` is what lets
/// `&dyn Source` be the `S` in question.
impl<T: Source + ?Sized> Source for &T {
    fn read(&self, path: &str) -> Result<Vec<u8>, Missing> {
        (**self).read(path)
    }

    fn list(&self) -> Result<Vec<String>, Missing> {
        (**self).list()
    }

    // `write` is *not* forwarded, and cannot be: `&mut self` here is a mutable
    // borrow of the reference rather than of what it points at, so there is no
    // `&mut T` to hand on. Inheriting the refusing default is the honest answer
    // and not a gap -- a caller holding a shared borrow of a source was never in
    // a position to write through it, and this is where that shows up.

    // Forwarded rather than left to the default, which is not a formality: the
    // default reads the file and throws the bytes away, so a `&Memory` that
    // inherited it would answer a question the map can settle with a key lookup
    // by copying a level out of a `BTreeMap` first.
    fn exists(&self, path: &str) -> bool {
        (**self).exists(path)
    }
}

/// Nothing at all, for a game whose levels are constants.
///
/// A `Level::load` that ignores its source -- pong's court is written in the
/// source and read from no file -- still has to be handed one, and this is what
/// a caller with nothing to hand over hands over. Every read fails; that is the
/// honest answer for a source with no files in it, and a `load` that ignores it
/// never asks. Writes fail too, through the trait's own default: a source that
/// is nothing at all is the read-only case in its purest form.
impl Source for () {
    fn read(&self, path: &str) -> Result<Vec<u8>, Missing> {
        Err(Missing::new(path))
    }

    fn list(&self) -> Result<Vec<String>, Missing> {
        Ok(Vec::new())
    }

    fn exists(&self, _path: &str) -> bool {
        false
    }
}
