#![doc = include_str!("../README.md")]
#![no_std]

// No `extern crate alloc`. Every function here takes a primitive integer and
// returns one, and there is nothing to allocate on the way.

mod length;
mod narrow;

pub use length::{
    bit_length_u32, bit_length_u64, bit_length_u128, magnitude_bits_i32, magnitude_bits_i64,
    magnitude_bits_i128,
};
pub use narrow::{narrow_i64, narrow_i128, try_narrow_i64, try_narrow_i128};
