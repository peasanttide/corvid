//! Clamping angles covering a quarter turn either side of zero.
//!
//! A pitch is an [angle](super::angle) that stops instead of wrapping:
//! [`MAX`](Pitch16::MAX) is exactly `+pi/2` and [`MIN`](Pitch16::MIN) is exactly
//! `-pi/2`, both inclusive, and arithmetic saturates at them. That is what a
//! camera's vertical look wants -- pitch past straight up should stay at straight
//! up, not flip the world over -- and equally what a latitude, an elevation, or a
//! slope wants.
//!
//! # Units shared with yaw
//!
//! A pitch uses the *same* scale as the [angle](super::angle) of matching width:
//! one turn is `2^BITS`, so [`Pitch16`] and [`Angle16`] resolve to the same
//! 1/65536 of a turn and convert between each other by
//! [`to_angle`](Pitch16::to_angle) with no arithmetic at all. A camera's two
//! angles are then directly comparable, and the trigonometry below is literally
//! the same code the wrapping angles use.
//!
//! The cost is one bit of the storage type: [`Pitch16`] only ever holds
//! `-16384 ..= 16384` of `i16`'s range. Bit patterns outside that are accepted --
//! keeping [`from_bits`](Pitch16::from_bits) the exact inverse of
//! [`to_bits`](Pitch16::to_bits), so `bytemuck` and `serde` stay faithful -- and
//! read as `+/-pi/2`, exactly as though they had been clamped. Arithmetic
//! canonicalizes, so no operation ever produces one.

use super::angle::{Angle8, Angle16, Angle32};
use super::factor::{Factor8, Factor16, Factor32};
use super::macros::{define_newtype, impl_binop, impl_neg, impl_num_traits_shared, impl_shared};
use super::point::I24F8;
use super::signed::{Signed8, Signed16, Signed32};
use crate::trig;
mod macros;
mod trig_impl;

use macros::define_pitch;
use trig_impl::define_pitch_trig;

define_pitch! {
    /// An 8-bit clamping pitch, covering `-pi/2 ..= pi/2`.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i8` |
    /// | Range | `-64 ..= 64`, denoting `-90 ..= 90` degrees |
    /// | Resolution | `1/256` turn, or 1.40625 degrees |
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::{Pitch8, Signed8};
    ///
    /// assert_eq!(Pitch8::from_degrees(90.0), Pitch8::MAX);
    /// assert_eq!(Pitch8::from_degrees(200.0), Pitch8::MAX);
    /// assert_eq!(Pitch8::MAX.sin(), Signed8::MAX);
    /// ```
    Pitch8(i8) {
        wide: i32,
        phase_shift: 24,
        angle: Angle8,
        signed: Signed8(i8),
        factor: Factor8,
    }
}

define_pitch_trig!(Pitch8, i8, i32, 24, Angle8, Signed8(i8), Factor8);

define_pitch! {
    /// A 16-bit clamping pitch, covering `-pi/2 ..= pi/2`.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i16` |
    /// | Range | `-16384 ..= 16384`, denoting `-90 ..= 90` degrees |
    /// | Resolution | `1/65536` turn, or about 0.0055 degrees |
    ///
    /// The camera pitch to reach for, paired with an [`Angle16`] yaw.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::{Angle16, Pitch16, Signed16};
    ///
    /// // Looking up past vertical stays at vertical.
    /// let mut pitch = Pitch16::from_degrees(85.0);
    /// pitch += Pitch16::from_degrees(20.0);
    /// assert_eq!(pitch, Pitch16::MAX);
    /// assert_eq!(pitch.to_degrees(), 90.0);
    ///
    /// // The endpoints are exact, in both directions.
    /// assert_eq!(Pitch16::MAX.sin(), Signed16::MAX);
    /// assert_eq!(Pitch16::MAX.cos(), Signed16::ZERO);
    /// assert_eq!(Pitch16::asin(Signed16::MAX), Pitch16::MAX);
    /// assert_eq!(Pitch16::asin(Signed16::ZERO), Pitch16::ZERO);
    ///
    /// // The cosine of a pitch is never negative, which is what makes it safe
    /// // to build a direction vector from.
    /// assert!(!Pitch16::MIN.cos().is_negative());
    ///
    /// // Yaw and pitch share a scale, so converting is free.
    /// assert_eq!(Pitch16::from_degrees(45.0).to_angle(), Angle16::from_degrees(45.0));
    /// assert_eq!(Pitch16::from_angle(Angle16::from_degrees(350.0)).to_degrees().round(), -10.0);
    /// ```
    Pitch16(i16) {
        wide: i32,
        phase_shift: 16,
        angle: Angle16,
        signed: Signed16(i16),
        factor: Factor16,
    }
}

define_pitch_trig!(Pitch16, i16, i32, 16, Angle16, Signed16(i16), Factor16);

define_pitch! {
    /// A 32-bit clamping pitch, covering `-pi/2 ..= pi/2`.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i32` |
    /// | Range | `-2^30 ..= 2^30`, denoting `-90 ..= 90` degrees |
    /// | Resolution | `1/2^32` turn, or about `8.4e-8` degrees |
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::{Pitch32, Signed32};
    ///
    /// let pitch = Pitch32::from_degrees(30.0);
    /// assert!((pitch.sin().to_f64() - 0.5).abs() < 1e-9);
    /// assert_eq!(Pitch32::asin(pitch.sin()).to_degrees().round(), 30.0);
    /// ```
    Pitch32(i32) {
        wide: i64,
        phase_shift: 0,
        angle: Angle32,
        signed: Signed32(i32),
        factor: Factor32,
    }
}

define_pitch_trig!(Pitch32, i32, i64, 0, Angle32, Signed32(i32), Factor32);
