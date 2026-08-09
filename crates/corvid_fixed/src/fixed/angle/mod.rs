//! Wrapping angles, and the trigonometry defined on them.
//!
//! An angle is a binary angle measurement: the storage type spans exactly one
//! turn, so a value `v` denotes `v / 2^BITS` turns and arithmetic wraps at the
//! full turn for free. There is no invalid angle, no normalization step, and no
//! accumulated drift from repeatedly adding to a heading.
//!
//! Wrapping is the only sensible overflow behavior on a circle, so these are the
//! one family with no `checked_` or `saturating_` operations. `+` and `-` wrap.
//!
//! # Trigonometry
//!
//! Trigonometry lives here rather than on the numeric families because an angle
//! is the only type that knows its own units. Two tiers are available:
//!
//! - [`sin`](Angle16::sin), [`cos`](Angle16::cos),
//!   [`sin_cos`](Angle16::sin_cos), [`tan`](Angle16::tan), and
//!   [`atan2`](Angle16::atan2) are the accurate tier. Sine and cosine are
//!   *correctly rounded*: the result is the same bit pattern you would get by
//!   rounding the true value to the output type, at every width. `tests/trig.rs`
//!   proves it for [`Angle8`] and [`Angle16`] by walking all 256 and all 65536
//!   inputs against `f64`. [`Angle32`] is finer than `f64` can referee, so it is
//!   held to a table of values computed in 80-digit arithmetic, plus a sweep of
//!   all 2^32 phases against the extended-precision path the implementation
//!   falls back to near a rounding boundary. That fallback costs [`Angle32`]
//!   about a tenth of its time and the narrower types nothing, since Q60 already
//!   rounds every input they have correctly.
//! - [`sin_fast`](Angle16::sin_fast), [`cos_fast`](Angle16::cos_fast), and
//!   [`atan2_fast`](Angle16::atan2_fast) trade accuracy for speed: worst-case
//!   error is `1.2e-3` for sine and `4.4e-3` radians for arctangent.
//!   Exact enough for [`Angle8`]/[`Signed8`], coarse for the
//!   wider types. They are also 32-bit clean -- no 64-bit intermediate, and no
//!   operation `WGSL` lacks -- so they transcribe directly into a shader, which
//!   is why [`atan2_fast`](Angle16::atan2_fast) takes `i32` coordinates where
//!   [`atan2`](Angle16::atan2) takes `i64`.
//!
//! Both tiers are `const` and use only integer arithmetic, so results are
//! bit-identical on every target -- a requirement for the deterministic
//! simulation this crate exists to serve.

use super::factor::{Factor8, Factor16, Factor32};
use super::macros::{
    define_newtype, impl_binop, impl_neg, impl_num_traits_shared, impl_num_traits_wrapping,
    impl_shared,
};
use super::point::I24F8;
use super::signed::{Signed8, Signed16, Signed32};
use crate::trig;
mod macros;
mod trig_impl;

use macros::define_angle;
use trig_impl::define_angle_trig;

define_angle! {
    /// An 8-bit wrapping angle.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `u8` |
    /// | Range | one full turn |
    /// | Resolution | `1/256` turn, or 1.40625 degrees |
    ///
    /// Coarse, but a whole heading in one byte -- and coarse enough that
    /// [`sin_fast`](Self::sin_fast) is already accurate to the last bit of its
    /// [`Signed8`] output.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::Angle8;
    ///
    /// assert_eq!(Angle8::from_degrees(90.0), Angle8::QUARTER_TURN);
    /// assert_eq!(Angle8::from_degrees(-90.0), Angle8::THREE_QUARTER_TURN);
    ///
    /// // A full turn wraps to zero, exactly.
    /// assert_eq!(Angle8::from_degrees(360.0), Angle8::ZERO);
    /// assert_eq!(Angle8::THREE_QUARTER_TURN + Angle8::QUARTER_TURN, Angle8::ZERO);
    /// ```
    Angle8(u8) {
        signed_repr: i8,
        phase_shift: 24,
        signed: Signed8(i8),
        factor: Factor8,
        pitch: Pitch8,
    }
}

define_angle_trig!(Angle8, u8, i8, 24, Signed8(i8), Factor8, Pitch8);

define_angle! {
    /// A 16-bit wrapping angle.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `u16` |
    /// | Range | one full turn |
    /// | Resolution | `1/65536` turn, or about 0.0055 degrees |
    ///
    /// The default choice: finer than a rendered pixel at any plausible
    /// distance, and small enough to hash and send over a wire without a second
    /// thought.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::{Angle16, Signed16};
    ///
    /// // Trigonometry is exact at the quarter turns.
    /// assert_eq!(Angle16::ZERO.sin(), Signed16::ZERO);
    /// assert_eq!(Angle16::ZERO.cos(), Signed16::MAX);
    /// assert_eq!(Angle16::QUARTER_TURN.sin(), Signed16::MAX);
    /// assert_eq!(Angle16::HALF_TURN.cos(), Signed16::MIN);
    ///
    /// // Angles wrap instead of overflowing, so headings never need clamping.
    /// let mut heading = Angle16::from_degrees(350.0);
    /// heading += Angle16::from_degrees(20.0);
    /// assert_eq!(heading.to_degrees().round(), 10.0);
    ///
    /// // The shortest arc is the wrapped difference, read as signed.
    /// let a = Angle16::from_degrees(10.0);
    /// let b = Angle16::from_degrees(350.0);
    /// assert_eq!(a.abs_diff(b).to_degrees().round(), 20.0);
    ///
    /// // atan2 takes any consistent units.
    /// assert_eq!(Angle16::atan2(1, 1), Angle16::from_degrees(45.0));
    /// assert_eq!(Angle16::atan2(-4, 0), Angle16::THREE_QUARTER_TURN);
    ///
    /// // Everything is const-evaluable, trigonometry included.
    /// const TILT: Angle16 = Angle16::from_degrees(30.0);
    /// const SINE: Signed16 = TILT.sin();
    /// assert!((SINE.to_f64() - 0.5).abs() < 1e-4);
    /// ```
    Angle16(u16) {
        signed_repr: i16,
        phase_shift: 16,
        signed: Signed16(i16),
        factor: Factor16,
        pitch: Pitch16,
    }
}

define_angle_trig!(Angle16, u16, i16, 16, Signed16(i16), Factor16, Pitch16);

define_angle! {
    /// A 32-bit wrapping angle.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `u32` |
    /// | Range | one full turn |
    /// | Resolution | `1/2^32` turn, or about `8.4e-8` degrees |
    ///
    /// Finer than `f32` can represent anywhere on the circle. Trigonometry costs
    /// the same as the narrower angles -- the shared core computes every result
    /// at 60 fractional bits regardless -- but the wider [`Signed32`] output is
    /// what makes that precision visible.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::{Angle32, Signed32};
    ///
    /// let third = Angle32::from_turns(1.0 / 3.0);
    /// assert!((third.to_degrees() - 120.0).abs() < 1e-6);
    ///
    /// // Three thirds of a turn come back to (almost) zero: the residue is the
    /// // rounding of a third of a turn, not accumulated drift.
    /// let full = third + third + third;
    /// assert!(full.abs_diff(Angle32::ZERO).to_bits() <= 2);
    ///
    /// assert_eq!(Angle32::QUARTER_TURN.cos(), Signed32::ZERO);
    /// ```
    Angle32(u32) {
        signed_repr: i32,
        phase_shift: 0,
        signed: Signed32(i32),
        factor: Factor32,
        pitch: Pitch32,
    }
}

define_angle_trig!(Angle32, u32, i32, 0, Signed32(i32), Factor32, Pitch32);
