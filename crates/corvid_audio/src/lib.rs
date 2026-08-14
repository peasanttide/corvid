#![doc = include_str!("../README.md")]

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
mod timbre;
#[cfg(feature = "device")]
mod unavailable;
mod voice;

pub use catalogue::Catalogue;
pub use extract::notes;
pub use heard::Heard;
pub use mixer::{Mixer, Note};
pub use timbre::Timbre;

// One pair of names under both configurations. Without the feature they are
// the stub in `silent`, whose `open` reports the same kind of thing a machine
// with no speakers does -- so a caller opens a sound card the same way whichever
// build it is in, and finds out at runtime rather than at compile time.
#[cfg(feature = "device")]
pub use device::Audio;
#[cfg(not(feature = "device"))]
pub use silent::{Audio, Unavailable};
#[cfg(feature = "device")]
pub use unavailable::Unavailable;
