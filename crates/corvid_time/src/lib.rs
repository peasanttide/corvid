#![doc = include_str!("../README.md")]
#![no_std]

// `Clock::wall` is the only thing here that asks the operating system
// anything, and `std` is the only feature that adds API -- the clock itself, in
// its stepping mode, is `no_std` like everything else. Nothing here reaches
// past `core` otherwise, and there is no allocation in this crate at all.
#[cfg(feature = "std")]
extern crate std;

mod clock;
mod span;
mod step;
mod tick;
mod ticks;

pub use clock::{Clock, Elapsed};
pub use span::TickSpan;
pub use step::Step;
pub use tick::Tick;
pub use ticks::Ticks;

/// Re-exported so a period, a frame time and a clock reading are all the same
/// type from the same place. It lives in `core::time` rather than `std::time`,
/// which is easy to forget in a `no_std` crate and is the only reason this
/// re-export exists.
pub use core::time::Duration;
