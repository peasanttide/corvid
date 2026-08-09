//! Signed normalized values covering `-1.0 ..= 1.0`.
//!
//! A value `v` denotes `v / MAX`, so [`MAX`](Signed8::MAX) is exactly `1.0` and
//! [`MIN`](Signed8::MIN) is exactly `-1.0`. This is the GPU `SNORM` convention,
//! matching `wgpu`'s `Snorm8`/`Snorm16` formats.
//!
//! # The denormal
//!
//! `SNORM` spends one bit pattern twice. For [`Signed8`], both `-128` and `-127`
//! denote `-1.0`, because the range is clamped rather than wrapped. That is a
//! genuine wart, and left alone it would break `Hash`/`Eq` agreement -- two
//! values equal as numbers but unequal as bits -- which Corvid's state hashing
//! depends on.
//!
//! The resolution here:
//!
//! - [`MIN`](Signed8::MIN) is `-127`, the canonical `-1.0`.
//! - [`from_bits`](Signed8::from_bits) accepts `-128` unchanged, so it stays the
//!   exact inverse of [`to_bits`](Signed8::to_bits) and `bytemuck` casts of
//!   arbitrary bytes remain faithful.
//! - `PartialEq`, `Ord`, and `Hash` compare and hash the canonical form, so
//!   `-128` and `-127` are one value to every collection and every state hash.
//! - Every arithmetic operation canonicalizes its inputs, so `-128` cannot
//!   propagate: no result of arithmetic is ever denormal.
//! - [`canonicalize`](Signed8::canonicalize) makes the fold explicit when a
//!   caller wants the bits themselves normalized.

use super::factor::{Factor8, Factor16, Factor32};
use super::macros::{
    define_newtype, impl_binop, impl_neg, impl_num_traits_arith, impl_num_traits_shared, impl_one,
    impl_shared,
};

mod macros;

use macros::define_signed;

define_signed! {
    /// An 8-bit signed normalized value covering `-1.0 ..= 1.0`.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i8` |
    /// | Range | `-1.0 ..= 1.0`, with `+/-127` denoting `+/-1.0` |
    /// | Resolution | `1/127`, or about `0.0079` |
    ///
    /// Bit-compatible with `wgpu`'s `Snorm8` formats. The output type of
    /// [`Angle8`](crate::Angle8)'s trigonometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::Signed8;
    ///
    /// assert_eq!(Signed8::MIN.to_f64(), -1.0);
    /// assert_eq!(Signed8::MAX.to_f64(), 1.0);
    /// assert_eq!(-Signed8::MIN, Signed8::MAX);
    ///
    /// // The two encodings of -1.0 are one value.
    /// let denormal = Signed8::from_bits(-128);
    /// assert!(denormal.is_denormal());
    /// assert_eq!(denormal, Signed8::MIN);
    /// assert_eq!(denormal.to_f64(), -1.0);
    /// assert_eq!(denormal.canonicalize().to_bits(), -127);
    ///
    /// // ... but from_bits still reports what it was handed.
    /// assert_eq!(denormal.to_bits(), -128);
    /// ```
    Signed8(i8) {
        wide: i32,
        uwide: u32,
        factor: Factor8,
    }
}

define_signed! {
    /// A 16-bit signed normalized value covering `-1.0 ..= 1.0`.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i16` |
    /// | Range | `-1.0 ..= 1.0`, with `+/-32767` denoting `+/-1.0` |
    /// | Resolution | `1/32767`, or about `3.1e-5` |
    ///
    /// Bit-compatible with `wgpu`'s `Snorm16` formats. The output type of
    /// [`Angle16`](crate::Angle16)'s trigonometry, and the type to reach for
    /// when storing a direction or a normal.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::Signed16;
    ///
    /// // Multiplication is closed over [-1, 1], so it never fails.
    /// assert_eq!(Signed16::MIN * Signed16::MIN, Signed16::MAX);
    /// assert_eq!(Signed16::MAX * Signed16::MIN, Signed16::MIN);
    ///
    /// // Addition saturates instead.
    /// assert_eq!(Signed16::MAX + Signed16::MAX, Signed16::MAX);
    /// assert_eq!(Signed16::MAX.checked_add(Signed16::MAX), None);
    ///
    /// assert_eq!(Signed16::from_f64(-0.25).abs(), Signed16::from_f64(0.25));
    /// assert_eq!(Signed16::from_f64(-0.25).sqrt(), Signed16::ZERO);
    /// assert_eq!(Signed16::from_f64(-0.25).checked_sqrt(), None);
    /// ```
    Signed16(i16) {
        wide: i64,
        uwide: u64,
        factor: Factor16,
    }
}

define_signed! {
    /// A 32-bit signed normalized value covering `-1.0 ..= 1.0`.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i32` |
    /// | Range | `-1.0 ..= 1.0`, with `+/-2147483647` denoting `+/-1.0` |
    /// | Resolution | about `4.7e-10` |
    ///
    /// Multiplication and division use a 128-bit intermediate, so they cost
    /// more than the narrower types. Round-tripping through `f32` is lossy --
    /// use `f64`.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::Signed32;
    ///
    /// let v = Signed32::from_f64(-0.5);
    /// assert_eq!(v.to_bits(), -1_073_741_824);
    /// assert_eq!(Signed32::from_f64(v.to_f64()), v);
    /// assert_eq!(v.signum(), Signed32::MIN);
    /// ```
    Signed32(i32) {
        wide: i128,
        uwide: u128,
        factor: Factor32,
    }
}
