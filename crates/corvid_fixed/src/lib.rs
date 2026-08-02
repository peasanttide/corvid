#![doc = include_str!("../README.md")]
#![no_std]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    reason = "narrowing is this crate's subject matter; every cast is preceded by a range check, a saturating conversion, or a mask, and the exhaustive tests cover the boundaries"
)]

// `arbitrary`'s derive macro emits `::std` paths, so that feature — and only
// that feature — pulls in std. Nothing else here reaches past `core`.
#[cfg(feature = "arbitrary")]
extern crate std;

mod fixed;
mod trig;

pub use fixed::{angle, factor, pitch, point, signed};

pub use angle::{Angle8, Angle16, Angle32};
pub use factor::{Factor8, Factor16, Factor32};
pub use pitch::{Pitch8, Pitch16, Pitch32};
pub use point::{I0F8, I2F30, I8F8, I16F16, I24F8, I48F16};
pub use signed::{Signed8, Signed16, Signed32};
