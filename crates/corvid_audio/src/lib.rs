#![doc = include_str!("../README.md")]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the modules here are private, so pub(crate) and pub are equivalent — pub(crate) is the one that says what is meant, and it is what rustc's unreachable_pub asks for"
)]

// No `no_std`. This crate talks to a sound card and hands a mixer to a thread
// the operating system owns.

mod catalogue;
#[cfg(feature = "device")]
mod device;
mod extract;
mod heard;
mod mixer;
#[cfg(not(feature = "device"))]
mod silent;
mod voice;

pub use catalogue::Catalogue;
pub use extract::notes;
pub use heard::Heard;
pub use mixer::{Mixer, Note};
pub use voice::Timbre;

// One pair of names under both configurations. Without the feature they are
// the stub in `silent`, whose `open` reports the same kind of thing a machine
// with no speakers does — so a caller opens a sound card the same way whichever
// build it is in, and finds out at runtime rather than at compile time.
#[cfg(feature = "device")]
pub use device::{Audio, Unavailable};
#[cfg(not(feature = "device"))]
pub use silent::{Audio, Unavailable};
