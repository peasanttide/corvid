#![doc = include_str!("../README.md")]
#![no_std]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the modules here are private, so pub(crate) and pub are equivalent -- pub(crate) is the one that says what is meant, and it is what rustc's unreachable_pub asks for"
)]

// A log is a `Vec`, a snapshot ring is a `Vec` of states, and a level arrives
// behind an `Arc`. This crate needs an allocator and nothing else: there is no
// `std` here under any feature, and in particular no clock.
extern crate alloc;

mod encode;
mod log;
mod open;
mod opening;
mod replay_error;
mod schema;
mod seek;
mod session;
mod snapshots;
mod trace;

pub use log::{ActionLog, Refused};
pub use open::Opens;
pub use opening::{Opening, Profile, Seed};
pub use replay_error::{Forget, Load, Shape};
pub use schema::Schema;
pub use seek::Unreachable;
pub use session::Session;
pub use snapshots::Snapshots;
pub use trace::HashTrace;
