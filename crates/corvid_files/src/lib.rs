#![doc = include_str!("../README.md")]
#![no_std]

// The one allocation here is a file's bytes, and a filesystem that cannot hand
// back bytes is not one. Nothing else in this crate needs an operating system —
// which is the point of it: `corvid_behavior` is `no_std` and names `Source` in
// `Level::load`'s signature, so the trait has to live below `std` even though
// every real implementation of it is above.
extern crate alloc;

mod memory;
mod source;

pub use memory::Memory;
pub use source::{Malformed, Missing, ReadOnly, Source};
