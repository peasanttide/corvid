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
#![allow(
    clippy::many_single_char_names,
    reason = "x, y, z and the octahedral u, v, w are the names this subject matter uses; spelling them out would obscure the formulae rather than clarify them"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "these modules are private, so pub(crate) and pub are equivalent — pub(crate) is the one that says what is meant, and keeps the helpers from looking like API if a module is ever made public"
)]

// `arbitrary`'s derive macro emits `::std` paths, so that feature — and only
// that feature — pulls in std. Nothing else here reaches past `core`.
#[cfg(feature = "arbitrary")]
extern crate std;

mod convert;
#[cfg(any(feature = "mint", feature = "nalgebra"))]
mod interop;
mod oct;
mod point;

pub use convert::OutOfRange;
pub use oct::OctDirection;
pub use point::{
    Direction, FinePoint, GlobalFinePoint, GlobalPoint, direction, finepoint, globalfinepoint,
    globalpoint,
};
