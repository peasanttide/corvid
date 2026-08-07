//! Writing a run down: four kinds of file, two encodings, and one of them is
//! a picture.
//!
//! # Why one of them is a picture
//!
//! Drawing is raw `wgpu`, and `wgpu` calls cannot be diffed — so what a run
//! with a device can be compared on is the pixels a real adapter produced.
//!
//! That is a weak golden and the crate documentation says how weak.  In short:
//! rasterisation differs between drivers, so a PNG is compared with a tolerance
//! and its exact-match arm is pinned to one adapter. The bit-exact golden is
//! the hash trace, which is the one that matters — a picture that agreed while
//! the simulation diverged would say nothing at all.

use std::{
    fs,
    path::{Path, PathBuf},
};

use corvid_replay::HashTrace;
use corvid_sound::AudioFrame;
use corvid_time::Tick;

use crate::Error;

/// The subdirectory one PNG per displayed frame goes in.
///
/// Only a run with an offscreen renderer writes into it. A headless run has no
/// adapter and a windowed run has no texture left to read, so both leave it
/// empty — created, because a directory that is sometimes absent is a second
/// thing for a comparison to be confused by.
pub(crate) const FRAMES: &str = "frames";

/// The subdirectory one serialized audio frame per displayed frame goes in.
pub(crate) const AUDIO: &str = "audio";

/// The file the [`HashTrace`] goes in.
pub(crate) const TRACE: &str = "trace";

/// The file the [`Session`](corvid_replay::Session) goes in.
pub(crate) const SESSION: &str = "session";

/// The extension a frame is written under.
pub(crate) const PNG: &str = "png";

/// A directory a run writes itself into.
///
/// Everything but the pictures goes through `corvid_wire`, which is the
/// workspace's one encoding and the one
/// [`Session::load`](corvid_replay::Session::load) reads.
#[derive(Clone, Debug)]
pub(crate) struct Capture {
    /// The directory everything is written under.
    root: PathBuf,
}

impl Capture {
    /// Creates the directory and the two subdirectories a frame goes in.
    ///
    /// An existing directory is written into rather than emptied. A run that
    /// captures over the top of a shorter run leaves the longer run's frames
    /// behind it, which is a hazard worth knowing about and is the caller's to
    /// avoid — this crate will not remove a directory somebody named.
    pub(crate) fn open(root: PathBuf) -> Result<Self, Error> {
        for directory in [root.as_path(), &root.join(FRAMES), &root.join(AUDIO)] {
            fs::create_dir_all(directory).map_err(|why| Error::Wrote {
                path: directory.to_path_buf(),
                why,
            })?;
        }
        Ok(Self { root })
    }

    /// Writes one displayed frame's picture, if there is one, and its audio
    /// frame.
    ///
    /// Both are named for the tick the frame's `current` state is at. A run
    /// that displays several frames between two ticks writes each of them over
    /// the last, so what a capture holds is the final frame at each tick — see
    /// the crate documentation.
    ///
    /// `png` arrives already encoded rather than as an image, and that is not
    /// laziness: `corvid_render` is an optional dependency, so a build without
    /// the `render` feature has no image type to name here at all. The one
    /// caller that has a picture is the one that has a device.
    pub(crate) fn frame(
        &self,
        at: Tick,
        png: Option<&[u8]>,
        audio: &AudioFrame,
    ) -> Result<(), Error> {
        let name = at.to_string();
        if let Some(bytes) = png {
            write(&self.root.join(FRAMES).join(format!("{name}.{PNG}")), bytes)?;
        }
        put(&self.root.join(AUDIO).join(&name), "an audio frame", audio)
    }

    /// Writes the hash trace and the session, which are what a capture is
    /// replayed from.
    pub(crate) fn close(&self, session: &[u8], marks: &HashTrace) -> Result<(), Error> {
        put(&self.root.join(TRACE), "a hash trace", marks)?;
        write(&self.root.join(SESSION), session)
    }
}

/// Encodes one value and writes it where it goes.
fn put<T: serde::Serialize>(path: &Path, what: &'static str, value: &T) -> Result<(), Error> {
    let bytes = corvid_wire::encode(value).map_err(|why| Error::Encoded { what, why })?;
    write(path, &bytes)
}

/// Writes bytes to a path, naming the path in whatever goes wrong.
///
/// `std::io::Error` does not carry the path it was about, so a capture that
/// failed would otherwise report "permission denied" and leave the reader to
/// work out which of a thousand files it was.
fn write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    fs::write(path, bytes).map_err(|why| Error::Wrote {
        path: path.to_path_buf(),
        why,
    })
}
