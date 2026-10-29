#![doc = include_str!("../README.md")]

// No `no_std`. This crate decodes a SoundFont with `std`'s allocator and
// threads, and hands a stream to a thread the operating system owns.

mod bank;
#[cfg(feature = "device")]
mod device;
mod error;
mod player;
#[cfg(not(feature = "device"))]
mod silent;
mod song;
#[cfg(feature = "device")]
mod unavailable;

pub use bank::Bank;
pub use error::Error;
pub use player::Player;
pub use song::Song;

// One pair of names under both configurations, as `corvid_audio` does it: a
// caller opens the music the same way in either build and finds out at run
// time whether there is a device.
#[cfg(feature = "device")]
pub use device::Music;
#[cfg(not(feature = "device"))]
pub use silent::{Music, Unavailable};
#[cfg(feature = "device")]
pub use unavailable::Unavailable;
