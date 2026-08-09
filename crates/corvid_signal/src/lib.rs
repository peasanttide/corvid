#![doc = include_str!("../README.md")]
// No `#![no_std]`. Every crate in the simulation and client rings has one, and
// this is the first that cannot: the thing it carries state between is threads,
// and a thread is a platform's to give -- `Mutex`, `Condvar` and the parking a
// `Condvar` is built on are all `std`. The README's second section says why the
// boundary is here rather than one crate further in.

mod channel;
mod seen;

pub use channel::{Emitter, Watch, channel};
pub use seen::Seen;
