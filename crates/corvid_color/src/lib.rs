#![doc = include_str!("../README.md")]
#![no_std]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the modules here are private, so pub(crate) and pub are equivalent -- pub(crate) is the one that says what is meant, and it is what rustc's unreachable_pub asks for"
)]

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
