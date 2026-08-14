//! The sound card, in a build that has no backend to open one with.
//!
//! # Why a stub rather than nothing
//!
//! The `device` feature decides whether this crate links `cpal`, and that is a
//! decision about binary weight. It is **not** a decision about what the code
//! around it looks like: a runtime that opens a sound card already has to cope
//! with a machine that has none -- a container, a build machine, a remote
//! desktop -- and reports it by carrying [`None`] and playing the game silently.
//!
//! A build with no backend is the same situation arriving earlier. So this
//! module answers it the same way, and the caller's `cfg` disappears:
//! [`Audio::open`] reports [`Unavailable`] here exactly as the real one does on
//! a machine with no speakers, and `corvid_app::screen` holds one field and one
//! code path either way.
//!
//! # It is uninhabited
//!
//! [`Audio`] is an empty enum, so [`open`](Audio::open) returning [`Err`] is not
//! a convention this module follows but the only thing it *can* do -- there is no
//! value of the type to return. [`hear`](Audio::hear) is likewise a method
//! nothing can call, which is the strongest available statement that a build
//! without the feature plays no sound.

use corvid_sound::AudioFrame;

use crate::Catalogue;

/// Why a device would not play, when the answer is always the same one.
///
/// [`non_exhaustive`](https://doc.rust-lang.org/reference/attributes/type_system.html)
/// like its counterpart under the `device` feature, so that a caller matching
/// on either writes the same wildcard arm.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Unavailable {
    /// This build has no audio backend compiled into it.
    ///
    /// The `device` feature is what adds one. Nothing about the machine is
    /// being reported here -- a machine with speakers running this build gets
    /// this too.
    #[error("this build has no audio backend; the `device` feature adds one")]
    NoBackend,
}

/// The sound card that this build cannot open.
///
/// Uninhabited: there is no backend to make one out of, so there is no value of
/// this type anywhere in such a build. What that buys is that every caller
/// keeps its `Option<Audio>` field and its one call to [`hear`](Self::hear)
/// under both configurations, and the compiler removes the whole path.
#[derive(Debug)]
pub enum Audio {}

impl Audio {
    /// Reports that this build has no backend.
    ///
    /// # Errors
    ///
    /// Always [`Unavailable::NoBackend`]. A caller that treats it the way it
    /// treats a machine with no output device -- carry [`None`], play silently,
    /// say so once -- needs no change between the two builds, which is the
    /// point.
    ///
    /// Not `const`, because the catalogue it is handed owns a map and dropping
    /// one is not something a constant may do. It takes the catalogue by value
    /// anyway, so that the signature matches the real
    /// [`open`](crate::Audio::open) and a caller does not discover the
    /// difference by having its argument left behind.
    pub fn open(_catalogue: Catalogue) -> Result<Self, Unavailable> {
        Err(Unavailable::NoBackend)
    }

    /// Plays what a frame describes, which cannot happen.
    ///
    /// `self` is uninhabited, so this body is a match with no arms and the call
    /// is unreachable rather than empty.
    pub fn hear(&mut self, _frame: &AudioFrame) {
        match *self {}
    }
}
