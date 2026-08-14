#![doc = include_str!("../README.md")]
#![no_std]

// No `extern crate alloc`. A colour is four numbers and a palette is an array
// of them; nothing here grows, and nothing here has a length that is not known
// where it is written.

mod linear;
mod oklab;
mod rgba;
mod transfer;

pub use linear::LinearRgba;
pub use oklab::{Oklab, Oklch};
pub use rgba::Rgba8;
pub use transfer::{decode, encode};
