//! The sound card, in a build with no backend to open one with.
//!
//! The same arrangement as `corvid_audio`'s stub: a run on a machine with no
//! speakers already has to carry `None` and play silently, so a build without
//! the `device` feature answers the same way and the caller's code does not
//! change between the two. [`Music`] is uninhabited, so [`Music::open`]
//! failing is the only thing it can do.

use crate::{Bank, Error, Song};

/// Why the music would not play, when the answer is always the same one.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Unavailable {
    /// This build has no audio backend; the `device` feature adds one.
    #[error("this build has no audio backend; the `device` feature adds one")]
    NoBackend,
}

/// The sound card this build cannot open: there is no value of this type.
#[derive(Debug)]
pub enum Music {}

impl Music {
    /// Reports that this build has no backend, without calling `load`.
    ///
    /// # Errors
    ///
    /// Always [`Unavailable::NoBackend`].
    pub fn open<F>(_load: F) -> Result<Self, Unavailable>
    where
        F: FnOnce() -> Result<Bank, Error> + Send + 'static,
    {
        Err(Unavailable::NoBackend)
    }

    /// The device's rate, which no device has.
    #[must_use]
    pub const fn rate(&self) -> u32 {
        match *self {}
    }

    /// Starts a song, which cannot happen.
    pub fn play(&self, _song: Song) {
        match *self {}
    }

    /// Stops the song, which cannot happen.
    pub fn stop(&self) {
        match *self {}
    }

    /// Sets the gain, which cannot happen.
    pub fn set_gain(&self, _gain: f32) {
        match *self {}
    }

    /// Whether a song is playing, which cannot be asked.
    #[must_use]
    pub const fn playing(&self) -> bool {
        match *self {}
    }
}
