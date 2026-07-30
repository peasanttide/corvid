//! Signed normalized values covering `-1.0 ..= 1.0`.
//!
//! A value `v` denotes `v / MAX`, so [`MAX`](Signed8::MAX) is exactly `1.0` and
//! [`MIN`](Signed8::MIN) is exactly `-1.0`. This is the GPU `SNORM` convention,
//! matching `wgpu`'s `Snorm8`/`Snorm16` formats.
//!
//! # The denormal
//!
//! `SNORM` spends one bit pattern twice. For `Signed8`, both `-128` and `-127`
//! denote `-1.0`, because the range is clamped rather than wrapped. That is a
//! genuine wart, and left alone it would break `Hash`/`Eq` agreement — two
//! values equal as numbers but unequal as bits — which Corvid's state hashing
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

/// Generates a signed normalized type.
///
/// `wide` must hold twice the square of `repr::MAX`, the largest intermediate
/// that multiplication, division, and square root produce.
macro_rules! define_signed {
    (
        $(#[$attr:meta])*
        $name:ident($repr:ty) {
            wide: $wide:ty,
            uwide: $uwide:ty,
            factor: $factor:ident,
        }
    ) => {
        define_newtype! {
            $(#[$attr])*
            $name($repr)
        }

        impl $name {
            /// The largest value, exactly `1.0`.
            pub const MAX: Self = Self(<$repr>::MAX);

            /// The smallest value, exactly `-1.0`.
            ///
            /// One less than the storage type's minimum: see the
            /// [module documentation](self) on the `SNORM` denormal.
            pub const MIN: Self = Self(-<$repr>::MAX);

            /// The difference between adjacent representable values.
            pub const DELTA: Self = Self(1);

            /// The bit pattern denoting `1.0`, widened for arithmetic.
            const SCALE: $wide = <$repr>::MAX as $wide;

            /// Converts from `f64`, clamping to `-1.0 ..= 1.0`.
            ///
            /// Halfway cases round away from zero, values outside the range
            /// clamp, and `NaN` becomes [`ZERO`](Self::ZERO). The result is
            /// always canonical.
            #[must_use]
            #[inline]
            pub const fn from_f64(value: f64) -> Self {
                let scaled = value * <$repr>::MAX as f64;
                let rounded = if scaled >= 0.0 { scaled + 0.5 } else { scaled - 0.5 };
                // The cast clamps at the storage bounds and sends NaN to zero;
                // canonicalizing then folds the one denormal it can produce.
                Self(rounded as $repr).canonicalize()
            }

            /// Converts from `f64`, or returns `None` if the value cannot be
            /// represented.
            ///
            /// "Cannot be represented" means more than half a step outside
            /// `-1.0 ..= 1.0`: a value that rounds cleanly onto
            /// [`MIN`](Self::MIN) or [`MAX`](Self::MAX) converts, and anything
            /// past that is rejected. `NaN` returns `None`.
            #[must_use]
            #[inline]
            pub const fn checked_from_f64(value: f64) -> Option<Self> {
                let scaled = value * <$repr>::MAX as f64;
                let limit = <$repr>::MAX as f64 + 0.5;
                if scaled > -limit && scaled < limit {
                    let rounded = if scaled >= 0.0 { scaled + 0.5 } else { scaled - 0.5 };
                    Some(Self(rounded as $repr))
                } else {
                    None
                }
            }

            /// The exact value as an `f64`.
            #[must_use]
            #[inline]
            pub const fn to_f64(self) -> f64 {
                self.cmp_key() as f64 / <$repr>::MAX as f64
            }

            /// The canonical bit pattern, folding the denormal `-1.0`.
            #[inline]
            const fn cmp_key(self) -> $repr {
                if self.0 < -<$repr>::MAX { -<$repr>::MAX } else { self.0 }
            }

            /// Replaces the denormal encoding of `-1.0` with the canonical one.
            ///
            /// A no-op for every other value. Arithmetic does this
            /// automatically; reach for it when handing raw bits to something
            /// that compares them itself.
            #[must_use]
            #[inline]
            pub const fn canonicalize(self) -> Self {
                Self(self.cmp_key())
            }

            /// Returns `true` if the bits are the denormal encoding of `-1.0`.
            #[must_use]
            #[inline]
            pub const fn is_denormal(self) -> bool {
                self.0 < -<$repr>::MAX
            }

            /// Clamps a wide bit pattern into `MIN ..= MAX`.
            #[inline]
            const fn saturate(wide: $wide) -> Self {
                if wide > Self::SCALE {
                    Self::MAX
                } else if wide < -Self::SCALE {
                    Self::MIN
                } else {
                    Self(wide as $repr)
                }
            }

            /// Checks that a wide bit pattern is in `MIN ..= MAX`.
            #[inline]
            const fn check(wide: $wide) -> Option<Self> {
                if wide > Self::SCALE || wide < -Self::SCALE {
                    None
                } else {
                    Some(Self(wide as $repr))
                }
            }

            /// Divides two wide values, rounding halfway cases away from zero.
            ///
            /// `denominator` must be positive.
            #[inline]
            const fn round_div(numerator: $wide, denominator: $wide) -> $wide {
                if numerator >= 0 {
                    (2 * numerator + denominator) / (2 * denominator)
                } else {
                    -((-2 * numerator + denominator) / (2 * denominator))
                }
            }

            /// Adds, returning `None` if the sum leaves `-1.0 ..= 1.0`.
            #[must_use]
            #[inline]
            pub const fn checked_add(self, rhs: Self) -> Option<Self> {
                Self::check(self.cmp_key() as $wide + rhs.cmp_key() as $wide)
            }

            /// Adds, clamping to [`MIN`](Self::MIN) or [`MAX`](Self::MAX).
            #[must_use]
            #[inline]
            pub const fn saturating_add(self, rhs: Self) -> Self {
                Self::saturate(self.cmp_key() as $wide + rhs.cmp_key() as $wide)
            }

            /// Subtracts, returning `None` if the result leaves `-1.0 ..= 1.0`.
            #[must_use]
            #[inline]
            pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
                Self::check(self.cmp_key() as $wide - rhs.cmp_key() as $wide)
            }

            /// Subtracts, clamping to [`MIN`](Self::MIN) or [`MAX`](Self::MAX).
            #[must_use]
            #[inline]
            pub const fn saturating_sub(self, rhs: Self) -> Self {
                Self::saturate(self.cmp_key() as $wide - rhs.cmp_key() as $wide)
            }

            /// Multiplies. Exact and total — `[-1, 1]` is closed under
            /// multiplication, so there is nothing to saturate or check.
            ///
            /// The product is formed at full width and rounded once, giving the
            /// representable value nearest the true product.
            #[must_use]
            #[inline]
            pub const fn mul(self, rhs: Self) -> Self {
                let product = (self.cmp_key() as $wide) * (rhs.cmp_key() as $wide);
                Self(Self::round_div(product, Self::SCALE) as $repr)
            }

            /// Multiplies. Always `Some`; provided for [`num_traits::CheckedMul`].
            #[must_use]
            #[inline]
            pub const fn checked_mul(self, rhs: Self) -> Option<Self> {
                Some(self.mul(rhs))
            }

            /// Multiplies. Never saturates; provided for [`num_traits::SaturatingMul`].
            #[must_use]
            #[inline]
            pub const fn saturating_mul(self, rhs: Self) -> Self {
                self.mul(rhs)
            }

            /// Divides, returning `None` on division by zero or a quotient
            /// outside `-1.0 ..= 1.0`.
            #[must_use]
            #[inline]
            pub const fn checked_div(self, rhs: Self) -> Option<Self> {
                if rhs.is_zero() {
                    return None;
                }
                let denominator = rhs.cmp_key() as $wide;
                let numerator = self.cmp_key() as $wide * Self::SCALE;
                let (numerator, denominator) = if denominator < 0 {
                    (-numerator, -denominator)
                } else {
                    (numerator, denominator)
                };
                Self::check(Self::round_div(numerator, denominator))
            }

            /// Divides, clamping to [`MIN`](Self::MIN) or [`MAX`](Self::MAX).
            ///
            /// Division by zero saturates in the direction of the numerator's
            /// sign, and `0 / 0` is [`ZERO`](Self::ZERO). The operation is
            /// total, which is what lets `/` exist without a panic.
            #[must_use]
            #[inline]
            pub const fn saturating_div(self, rhs: Self) -> Self {
                if rhs.is_zero() {
                    let numerator = self.cmp_key();
                    return if numerator > 0 {
                        Self::MAX
                    } else if numerator < 0 {
                        Self::MIN
                    } else {
                        Self::ZERO
                    };
                }
                let denominator = rhs.cmp_key() as $wide;
                let numerator = self.cmp_key() as $wide * Self::SCALE;
                let (numerator, denominator) = if denominator < 0 {
                    (-numerator, -denominator)
                } else {
                    (numerator, denominator)
                };
                Self::saturate(Self::round_div(numerator, denominator))
            }

            /// Negates. Exact and total, since the range is symmetric.
            #[must_use]
            #[inline]
            pub const fn neg(self) -> Self {
                Self(-self.cmp_key())
            }

            /// The absolute value. Exact and total.
            #[must_use]
            #[inline]
            pub const fn abs(self) -> Self {
                let bits = self.cmp_key();
                Self(if bits < 0 { -bits } else { bits })
            }

            /// Returns `-1.0`, `0.0`, or `1.0` according to the sign.
            #[must_use]
            #[inline]
            pub const fn signum(self) -> Self {
                let bits = self.cmp_key();
                if bits > 0 {
                    Self::MAX
                } else if bits < 0 {
                    Self::MIN
                } else {
                    Self::ZERO
                }
            }

            /// Returns `true` if this is less than zero.
            #[must_use]
            #[inline]
            pub const fn is_negative(self) -> bool {
                self.0 < 0
            }

            /// Returns `true` if this is greater than zero.
            #[must_use]
            #[inline]
            pub const fn is_positive(self) -> bool {
                self.0 > 0
            }

            /// The square root, rounded to the nearest representable value.
            ///
            /// Negative inputs return [`ZERO`](Self::ZERO) rather than
            /// panicking; use [`checked_sqrt`](Self::checked_sqrt) to detect
            /// them.
            #[must_use]
            #[inline]
            pub const fn sqrt(self) -> Self {
                if self.0 <= 0 {
                    return Self::ZERO;
                }
                let scaled = (self.0 as $uwide) * (<$repr>::MAX as $uwide);
                let root = scaled.isqrt();
                let rounded = if scaled - root * root > root { root + 1 } else { root };
                Self(rounded as $repr)
            }

            /// The square root, or `None` for a negative input.
            #[must_use]
            #[inline]
            pub const fn checked_sqrt(self) -> Option<Self> {
                if self.is_negative() { None } else { Some(self.sqrt()) }
            }

            #[doc = concat!("Linearly interpolates toward `to`, using a [`", stringify!($factor), "`] weight.")]
            ///
            /// Exact at both ends, and every intermediate result lies between
            /// the two endpoints, so this never overflows.
            #[must_use]
            #[inline]
            pub const fn lerp(self, to: Self, weight: $factor) -> Self {
                let from = self.cmp_key() as i128;
                let delta = to.cmp_key() as i128 - from;
                let numerator = delta * weight.to_bits() as i128;
                let denominator = $factor::MAX.to_bits() as i128;
                let scaled = if numerator >= 0 {
                    (2 * numerator + denominator) / (2 * denominator)
                } else {
                    -((-2 * numerator + denominator) / (2 * denominator))
                };
                Self((from + scaled) as $repr)
            }
        }

        impl_shared!($name, $repr, "");
        impl_binop!($name, Add::add, AddAssign::add_assign, saturating_add);
        impl_binop!($name, Sub::sub, SubAssign::sub_assign, saturating_sub);
        impl_binop!($name, Mul::mul, MulAssign::mul_assign, mul);
        impl_binop!($name, Div::div, DivAssign::div_assign, saturating_div);
        impl_neg!($name, neg);
        impl_num_traits_shared!($name);
        impl_num_traits_arith!($name);
        impl_one!($name, <$repr>::MAX);
    };
}

define_signed! {
    /// An 8-bit signed normalized value covering `-1.0 ..= 1.0`.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i8` |
    /// | Range | `-1.0 ..= 1.0`, with `±127` denoting `±1.0` |
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
    /// | Range | `-1.0 ..= 1.0`, with `±32767` denoting `±1.0` |
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
    /// | Range | `-1.0 ..= 1.0`, with `±2147483647` denoting `±1.0` |
    /// | Resolution | about `4.7e-10` |
    ///
    /// Multiplication and division use a 128-bit intermediate, so they cost
    /// more than the narrower types. Round-tripping through `f32` is lossy —
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
