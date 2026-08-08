#![doc = include_str!("../README.md")]
#![no_std]

// No `extern crate alloc`. A float is a float.

mod single;
pub mod wide;

pub use single::{
    abs, ceil, clamp, clamp_finite, copysign, cos, demote, floor, hypot, powi, recip, round, sin,
    sqrt, tan, trunc,
};

/// The `f32` constants, as [`core`] spells them.
///
/// Re-exported so that a crate reaching for a `PI` names this crate rather than
/// naming this crate *and* `core::f32::consts`, which is two imports for one
/// idea.
pub use core::f32::consts;
