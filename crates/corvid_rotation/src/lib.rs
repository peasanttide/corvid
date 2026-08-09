#![doc = include_str!("../README.md")]
#![no_std]
#![allow(
    clippy::redundant_pub_crate,
    reason = "these modules are private, so pub(crate) and pub are equivalent -- pub(crate) is the one that says what is meant, and keeps the helpers from looking like API if a module is ever made public"
)]
#![allow(
    clippy::many_single_char_names,
    reason = "x, y, z, w, m and v are the names this subject matter uses; spelling them out would obscure the formulae rather than clarify them"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    reason = "packing rotations into 32 and 64 bits is this crate's subject matter; every cast is preceded by a range check, a saturating conversion, or a bound stated in the comment above it"
)]

// `arbitrary`'s derive macro emits `::std` paths, so that feature -- and only
// that feature -- pulls in std. Nothing else here reaches past `core`.
#[cfg(feature = "arbitrary")]
extern crate std;

mod basis;
#[cfg(any(feature = "mint", feature = "nalgebra"))]
mod interop;
mod normalize;
mod ops;
mod rotation32;
mod rotation64;
mod versor;

pub use basis::Basis;
pub use rotation32::Rotation;
pub use rotation64::FineRotation;
pub use versor::Versor;
