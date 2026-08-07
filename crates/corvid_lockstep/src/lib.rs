#![doc = include_str!("../README.md")]
#![no_std]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the modules here are private, so pub(crate) and pub are equivalent — pub(crate) is the one that says what is meant, and it is what rustc's unreachable_pub asks for"
)]

// A log is a `Vec`, a snapshot is a whole state, and a level arrives behind an
// `Arc`. This crate needs an allocator and nothing else: there is no `std` here
// under any feature, no clock, and no socket.
extern crate alloc;

mod bisect;
mod confirm;
mod desync;
mod frame;
mod peer;
mod predict;
mod rollback;

#[cfg(feature = "dev")]
pub use bisect::bisect;
pub use bisect::{Bisect, Probes, TickProbes};
pub use confirm::{Budget, Frontier};
pub use desync::{Desync, FieldReport, Halt, Resync, Where};
pub use frame::{CATCHUP, Datagram, WINDOW};
pub use peer::Peer;
pub use predict::{Correction, Predicted, absorb, action_at, predict, row_at};
pub use rollback::{Advanced, Rolled};
