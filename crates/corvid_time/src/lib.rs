#![doc = include_str!("../README.md")]
#![no_std]

// `Wall` is the only thing here that asks the operating system anything, and
// `std` is the only feature that adds API. Nothing else reaches past `core` —
// there is no allocation in this crate at all.
#[cfg(feature = "std")]
extern crate std;

mod clock;
mod rate;
mod step;
mod tick;

#[cfg(feature = "std")]
pub use clock::Wall;
pub use clock::{Clock, Fake};
pub use rate::TickRate;
pub use step::Step;
pub use tick::Tick;

/// Re-exported so a period, a frame time and a clock reading are all the same
/// type from the same place. It lives in `core::time` rather than `std::time`,
/// which is easy to forget in a `no_std` crate and is the only reason this
/// re-export exists.
pub use core::time::Duration;
