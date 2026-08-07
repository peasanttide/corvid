#![doc = include_str!("../README.md")]
#![no_std]
#![allow(
    clippy::many_single_char_names,
    reason = "x, y, z, p and v are the names this subject matter uses; spelling them out would obscure the formulae rather than clarify them"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    reason = "narrowing between position widths is this crate's subject matter; every cast is preceded by a range check or a saturating conversion"
)]

// `arbitrary`'s derive macro emits `::std` paths, so that feature — and only
// that feature — pulls in std. Nothing else here reaches past `core`.
#[cfg(feature = "arbitrary")]
extern crate std;

mod convert;
mod ops;
mod transform;

pub use convert::PositionOutOfRange;
pub use transform::{GlobalFineTransform, Transform, globalfinetransform, transform};
