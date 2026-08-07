#![doc = include_str!("../README.md")]
#![no_std]

// Written-down bytes are a `Vec<u8>`, and the reasons a value would not go into
// one are `String`s. Nothing here reaches past `alloc`.
extern crate alloc;

mod codec;
mod error;
pub mod golden;

pub use codec::{decode, encode};
pub use error::Error;
