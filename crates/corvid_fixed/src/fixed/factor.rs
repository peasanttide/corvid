//! Unsigned normalized factors covering `0.0 ..= 1.0`.
//!
//! A value `v` denotes `v / MAX`, so [`MAX`](Factor8::MAX) is exactly `1.0`.
//! This is the GPU `UNORM` convention: the bit patterns match `wgpu`'s
//! `Unorm8`/`Unorm16` vertex and texture formats, so factors cross the graphics
//! boundary without conversion.
//!
//! The alternative convention -- dividing by `2^BITS` so that conversion is a
//! shift -- buys cheaper arithmetic but cannot represent `1.0`, which makes a
//! blend weight of "fully on" impossible to express. Being exact at both ends
//! matters more here than saving a multiply, so multiplication pays for an
//! exact `round(a * b / MAX)` correction instead.

use super::macros::{
    define_newtype, impl_binop, impl_num_traits_arith, impl_num_traits_shared, impl_one,
    impl_shared,
};

/// Generates an unsigned normalized factor type.
///
/// `wide` must hold twice the square of `repr::MAX`, the largest intermediate
/// that multiplication, division, and square root produce.
macro_rules! define_factor {
    (
        $(#[$attr:meta])*
        $name:ident($repr:ty) {
            wide: $wide:ty,
        }
    ) => {
        define_newtype! {
            $(#[$attr])*
            $name($repr)
        }

        impl $name {
            /// The smallest value, `0.0`.
            pub const MIN: Self = Self(0);

            /// The largest value, exactly `1.0`.
            pub const MAX: Self = Self(<$repr>::MAX);

            /// The difference between adjacent representable values.
            pub const DELTA: Self = Self(1);

            /// The bit pattern denoting `1.0`, widened for arithmetic.
            const SCALE: $wide = <$repr>::MAX as $wide;

            /// Converts from `f64`, clamping to `0.0 ..= 1.0`.
            ///
            /// Halfway cases round away from zero. Values below zero clamp to
            /// [`ZERO`](Self::ZERO), values above one clamp to
            /// [`ONE`](Self::ONE), and `NaN` becomes [`ZERO`](Self::ZERO).
            #[must_use]
            #[inline]
            pub const fn from_f64(value: f64) -> Self {
                let scaled = value * <$repr>::MAX as f64;
                // A cast from f64 saturates at both ends and sends NaN to zero,
                // which is exactly the clamping this needs.
                Self((scaled + 0.5) as $repr)
            }

            /// Converts from `f64`, or returns `None` if the value cannot be
            /// represented.
            ///
            /// "Cannot be represented" means more than half a step outside
            /// `0.0 ..= 1.0`: a value that rounds cleanly onto
            /// [`ZERO`](Self::ZERO) or [`ONE`](Self::ONE) converts, and anything
            /// past that is rejected. `NaN` returns `None`.
            #[must_use]
            #[inline]
            pub const fn checked_from_f64(value: f64) -> Option<Self> {
                let scaled = value * <$repr>::MAX as f64;
                if scaled > -0.5 && scaled < <$repr>::MAX as f64 + 0.5 {
                    Some(Self((scaled + 0.5) as $repr))
                } else {
                    None
                }
            }

            /// The exact value as an `f64`.
            #[must_use]
            #[inline]
            pub const fn to_f64(self) -> f64 {
                self.0 as f64 / <$repr>::MAX as f64
            }

            /// The bit pattern used for comparison and hashing.
            #[inline]
            const fn cmp_key(self) -> $repr {
                self.0
            }

            /// Divides two non-negative wide values, rounding halfway up.
            #[inline]
            const fn round_div(numerator: $wide, denominator: $wide) -> $wide {
                (2 * numerator + denominator) / (2 * denominator)
            }

            /// Adds, returning `None` if the sum exceeds `1.0`.
            #[must_use]
            #[inline]
            pub const fn checked_add(self, rhs: Self) -> Option<Self> {
                match self.0.checked_add(rhs.0) {
                    Some(bits) => Some(Self(bits)),
                    None => None,
                }
            }

            /// Adds, clamping at `1.0`.
            #[must_use]
            #[inline]
            pub const fn saturating_add(self, rhs: Self) -> Self {
                Self(self.0.saturating_add(rhs.0))
            }

            /// Subtracts, returning `None` if the result would be negative.
            #[must_use]
            #[inline]
            pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
                match self.0.checked_sub(rhs.0) {
                    Some(bits) => Some(Self(bits)),
                    None => None,
                }
            }

            /// Subtracts, clamping at `0.0`.
            #[must_use]
            #[inline]
            pub const fn saturating_sub(self, rhs: Self) -> Self {
                Self(self.0.saturating_sub(rhs.0))
            }

            /// Multiplies. Exact and total -- the unit interval is closed under
            /// multiplication, so there is nothing to saturate or check.
            ///
            /// The product is formed at full width and rounded once, giving the
            /// representable value nearest the true product.
            #[must_use]
            #[inline]
            pub const fn mul(self, rhs: Self) -> Self {
                let product = (self.0 as $wide) * (rhs.0 as $wide);
                Self(Self::round_div(product, Self::SCALE) as $repr)
            }

            /// Multiplies. Always `Some`; provided for `num_traits::CheckedMul`.
            #[must_use]
            #[inline]
            pub const fn checked_mul(self, rhs: Self) -> Option<Self> {
                Some(self.mul(rhs))
            }

            /// Multiplies. Never saturates; provided for `num_traits::SaturatingMul`.
            #[must_use]
            #[inline]
            pub const fn saturating_mul(self, rhs: Self) -> Self {
                self.mul(rhs)
            }

            /// Divides, returning `None` on division by zero or a quotient
            /// above `1.0`.
            #[must_use]
            #[inline]
            pub const fn checked_div(self, rhs: Self) -> Option<Self> {
                if rhs.0 == 0 {
                    return None;
                }
                let quotient = Self::round_div(self.0 as $wide * Self::SCALE, rhs.0 as $wide);
                if quotient > Self::SCALE {
                    None
                } else {
                    Some(Self(quotient as $repr))
                }
            }

            /// Divides, clamping at `1.0`.
            ///
            /// Division by zero yields `1.0`, except `0 / 0` which is
            /// [`ZERO`](Self::ZERO). The operation is total, which is what lets
            /// `/` exist without a panic.
            #[must_use]
            #[inline]
            pub const fn saturating_div(self, rhs: Self) -> Self {
                if rhs.0 == 0 {
                    return if self.0 == 0 { Self::ZERO } else { Self::MAX };
                }
                let quotient = Self::round_div(self.0 as $wide * Self::SCALE, rhs.0 as $wide);
                if quotient > Self::SCALE {
                    Self::MAX
                } else {
                    Self(quotient as $repr)
                }
            }

            /// Returns `1.0 - self`, exactly.
            #[must_use]
            #[inline]
            pub const fn complement(self) -> Self {
                Self(<$repr>::MAX - self.0)
            }

            /// The square root, rounded to the nearest representable value.
            ///
            /// Always in range, since the square root of a value in the unit
            /// interval stays in the unit interval.
            #[must_use]
            #[inline]
            pub const fn sqrt(self) -> Self {
                let scaled = (self.0 as $wide) * Self::SCALE;
                let root = scaled.isqrt();
                let rounded = if scaled - root * root > root { root + 1 } else { root };
                Self(rounded as $repr)
            }

            /// Linearly interpolates toward `to`.
            ///
            /// Exact at both ends: a weight of [`ZERO`](Self::ZERO) returns
            /// `self` and [`ONE`](Self::ONE) returns `to`.
            #[must_use]
            #[inline]
            pub const fn lerp(self, to: Self, weight: Self) -> Self {
                let delta = to.0 as i128 - self.0 as i128;
                let numerator = delta * weight.0 as i128;
                let denominator = <$repr>::MAX as i128;
                let scaled = if numerator >= 0 {
                    (2 * numerator + denominator) / (2 * denominator)
                } else {
                    -((-2 * numerator + denominator) / (2 * denominator))
                };
                Self((self.0 as i128 + scaled) as $repr)
            }
        }

        impl_shared!($name, $repr, "");
        impl_binop!($name, Add::add, AddAssign::add_assign, saturating_add);
        impl_binop!($name, Sub::sub, SubAssign::sub_assign, saturating_sub);
        impl_binop!($name, Mul::mul, MulAssign::mul_assign, mul);
        impl_binop!($name, Div::div, DivAssign::div_assign, saturating_div);
        impl_num_traits_shared!($name);
        impl_num_traits_arith!($name);
        impl_one!($name, <$repr>::MAX);
    };
}

define_factor! {
    /// An 8-bit factor covering `0.0 ..= 1.0`.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `u8` |
    /// | Range | `0.0 ..= 1.0`, with `255` denoting `1.0` |
    /// | Resolution | `1/255`, or about `0.0039` |
    ///
    /// Bit-compatible with `wgpu`'s `Unorm8` formats.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::Factor8;
    ///
    /// assert_eq!(Factor8::ONE.to_f64(), 1.0);
    /// assert_eq!(Factor8::from_f64(1.0).to_bits(), 255);
    ///
    /// let half = Factor8::from_f64(0.5);
    /// assert_eq!(half.complement(), Factor8::from_bits(127));
    /// assert_eq!(Factor8::ONE * half, half);
    ///
    /// // Out-of-range input clamps; use the checked form to detect it.
    /// assert_eq!(Factor8::from_f64(-3.0), Factor8::ZERO);
    /// assert_eq!(Factor8::checked_from_f64(-3.0), None);
    /// ```
    Factor8(u8) {
        wide: u32,
    }
}

define_factor! {
    /// A 16-bit factor covering `0.0 ..= 1.0`.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `u16` |
    /// | Range | `0.0 ..= 1.0`, with `65535` denoting `1.0` |
    /// | Resolution | `1/65535`, or about `1.5e-5` |
    ///
    /// Bit-compatible with `wgpu`'s `Unorm16` formats.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::Factor16;
    ///
    /// // Multiplication is closed over the unit interval, so it never fails.
    /// let half = Factor16::from_f64(0.5);
    /// assert_eq!(half.to_bits(), 32768);
    /// assert_eq!((half * half).to_bits(), 16384);
    /// assert_eq!(Factor16::MAX * Factor16::MAX, Factor16::MAX);
    ///
    /// // Bit patterns survive a trip through f64 exactly.
    /// assert_eq!(Factor16::from_f64(half.to_f64()), half);
    ///
    /// assert_eq!(Factor16::from_bits(9).sqrt().to_bits(), 768);
    /// ```
    Factor16(u16) {
        wide: u64,
    }
}

define_factor! {
    /// A 32-bit factor covering `0.0 ..= 1.0`.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `u32` |
    /// | Range | `0.0 ..= 1.0`, with `4294967295` denoting `1.0` |
    /// | Resolution | about `2.3e-10` |
    ///
    /// Multiplication and division use a 128-bit intermediate, so they cost
    /// more than the narrower factors. Round-tripping through `f32` is lossy --
    /// use `f64`.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::Factor32;
    ///
    /// let t = Factor32::from_f64(0.25);
    /// assert_eq!(t.to_bits(), 1_073_741_824);
    /// assert_eq!(t.complement().to_bits(), 3_221_225_471);
    /// assert_eq!(Factor32::MAX.complement(), Factor32::ZERO);
    ///
    /// // Scaling by 2^32 - 1 rather than 2^32 means a quarter is not exactly a
    /// // quarter, but every bit pattern still round-trips through f64.
    /// assert_eq!(Factor32::from_f64(t.to_f64()), t);
    /// ```
    Factor32(u32) {
        wide: u128,
    }
}

impl Factor16 {
    /// The same factor at 32 bits, exactly.
    ///
    /// Both types denote `v / MAX`, so widening is a multiplication by
    /// `0x1_0001` rather than a shift: that is what carries
    /// [`MAX`](Factor16::MAX) to [`Factor32::MAX`] and leaves
    /// [`ZERO`](Factor16::ZERO) where it is. A shift by sixteen maps `1.0` to
    /// `1.0 - 2^-16` instead, which is the mistake this exists to stop anyone
    /// making twice.
    ///
    /// ```
    /// use corvid_fixed::{Factor16, Factor32};
    ///
    /// assert_eq!(Factor16::MAX.to_factor32(), Factor32::MAX);
    /// assert_eq!(Factor16::ZERO.to_factor32(), Factor32::ZERO);
    /// assert_eq!(Factor16::from_f64(0.5).to_factor32(), Factor32::from_f64(0.5));
    /// ```
    #[must_use]
    #[inline]
    pub const fn to_factor32(self) -> Factor32 {
        Factor32::from_bits(self.to_bits() as u32 * 0x1_0001)
    }
}
