#![doc = include_str!("../README.md")]
#![no_std]

// Written-down bytes are a `Vec<u8>`, and the reasons a value would not go into
// one are `String`s. Nothing here reaches past `alloc`.
extern crate alloc;

mod codec;
mod error;
mod faithful;
pub mod golden;

pub use codec::{CEILING, decode, encode};
pub use error::Error;
pub use faithful::{Unfaithful, round_trip_is_faithful};
