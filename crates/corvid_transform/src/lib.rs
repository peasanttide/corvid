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
pub use transform::{FineTransform, Transform};

// One name for downstream code to depend on: everything the layers below
// provide is re-exported here, so a game reaches for `corvid_transform` alone.
pub use corvid_fixed::{
    self as fixed, Angle8, Angle16, Angle32, Factor8, Factor16, Factor32, I0F8, I2F30, I8F8,
    I16F16, I24F8, I48F16, Pitch8, Pitch16, Pitch32, Signed8, Signed16, Signed32,
};
pub use corvid_rotation::{self as rotation, Basis, FineRotation, Rotation, Versor};
pub use corvid_vector::{self as vector, Direction, FinePoint, GlobalFinePoint, GlobalPoint};
