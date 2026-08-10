#![doc = include_str!("../README.md")]
#![no_std]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    reason = "narrowing between component widths is this crate's subject matter; every cast is preceded by a range check, a saturating conversion, or a shift whose bound is stated"
)]

// `arbitrary`'s derive macro emits `::std` paths, so that feature -- and only
// that feature -- pulls in std. Nothing else here reaches past `core`.
#[cfg(feature = "arbitrary")]
extern crate std;

mod convert;
#[cfg(any(feature = "mint", feature = "nalgebra"))]
mod interop;
mod point;

pub use convert::OutOfRange;
pub use point::{
    Direction, FinePoint, GlobalFinePoint, GlobalPoint, Volume, WideOffset, direction, finepoint,
    globalfinepoint, globalpoint,
};
