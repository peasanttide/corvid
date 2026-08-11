#![doc = include_str!("../README.md")]
#![no_std]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the modules here are private, so pub(crate) and pub are equivalent -- pub(crate) is the one that says what is meant, and it is what rustc's unreachable_pub asks for"
)]

// An `AudioFrame` holds three growable lists, so this crate needs an allocator.
// It needs nothing else: there is no `std` here under any feature.
extern crate alloc;

mod auralizer;
mod bus;
mod cue;
mod frame;
mod id;
mod source;

pub use auralizer::{Auralizer, Hearing};
pub use bus::Bus;
pub use cue::{Cue, CueId};
pub use frame::{AudioFrame, Listener};
pub use id::{BusId, SoundId, SourceId};
pub use source::Source;
