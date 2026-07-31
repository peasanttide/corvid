//! General-purpose signed fixed-point numbers.
//!
//! These follow the naming convention of the `fixed` crate: [`I8F8`] is signed,
//! with 8 integer bits and 8 fractional bits. A value `v` denotes
//! `v / 2^FRAC_BITS`, so conversion to and from the raw bits is a shift and
//! every value is an exact multiple of [`DELTA`](I8F8::DELTA).

use super::factor::{Factor8, Factor16, Factor32};
use super::macros::{
    define_newtype, impl_binop, impl_neg, impl_num_traits_arith, impl_num_traits_shared,
    impl_num_traits_wrapping, impl_one, impl_shared,
};

/// Generates a signed fixed-point type.
///
/// `wide` must hold the product of two `repr` values and twice a `repr` shifted
/// left by `frac`; `uwide` is its unsigned counterpart, used for square roots.
macro_rules! define_fixed_point {
    (
        $(#[$attr:meta])*
        $name:ident($repr:ty) {
            wide: $wide:ty,
            uwide: $uwide:ty,
            frac: $frac:expr,
            factor: $factor:ident,
        }
    ) => {
        define_newtype! {
            $(#[$attr])*
            $name($repr)
        }

        impl $name {
            /// Number of fractional bits.
            pub const FRAC_BITS: u32 = $frac;

            /// The largest representable value.
            pub const MAX: Self = Self(<$repr>::MAX);

            /// The smallest representable value.
            pub const MIN: Self = Self(<$repr>::MIN);

            /// The difference between adjacent representable values.
            pub const DELTA: Self = Self(1);

            /// The scale factor between bits and value.
            const SCALE: f64 = (1 << $frac) as f64;

            #[doc = concat!("Converts from `f64`, saturating at the bounds of `", stringify!($name), "`.")]
            ///
            /// Halfway cases round away from zero. Infinities saturate and
            /// `NaN` becomes [`ZERO`](Self::ZERO), so this is total.
            #[must_use]
            #[inline]
            pub const fn from_f64(value: f64) -> Self {
                Self(round_f64(value * Self::SCALE) as $repr)
            }

            /// Converts from `f64`, or returns `None` if the value cannot be
            /// represented.
            ///
            /// "Cannot be represented" means more than half a step outside the
            /// range: a value that rounds cleanly onto
            /// [`MIN`](Self::MIN) or [`MAX`](Self::MAX) converts, and anything
            /// past that is rejected. `NaN` returns `None`.
            ///
            /// This is the conversion to reach for when a value drifting out of
            /// range is a bug rather than something to clamp away.
            #[must_use]
            #[inline]
            pub const fn checked_from_f64(value: f64) -> Option<Self> {
                let scaled = value * Self::SCALE;
                // A NaN fails both comparisons, so it lands in None.
                if scaled > <$repr>::MIN as f64 - 0.5 && scaled < <$repr>::MAX as f64 + 0.5 {
                    Some(Self(round_f64(scaled) as $repr))
                } else {
                    None
                }
            }

            /// The exact value as an `f64`.
            ///
            /// Lossless for every type in this family: `f64` carries 53
            /// significant bits and the widest of these carries 32.
            #[must_use]
            #[inline]
            pub const fn to_f64(self) -> f64 {
                self.0 as f64 / Self::SCALE
            }

            /// The bit pattern used for comparison and hashing.
            #[inline]
            const fn cmp_key(self) -> $repr {
                self.0
            }

            /// Adds, returning `None` on overflow.
            #[must_use]
            #[inline]
            pub const fn checked_add(self, rhs: Self) -> Option<Self> {
                match self.0.checked_add(rhs.0) {
                    Some(bits) => Some(Self(bits)),
                    None => None,
                }
            }

            /// Adds, clamping to [`MIN`](Self::MIN) or [`MAX`](Self::MAX).
            #[must_use]
            #[inline]
            pub const fn saturating_add(self, rhs: Self) -> Self {
                Self(self.0.saturating_add(rhs.0))
            }

            /// Adds, wrapping around on overflow.
            #[must_use]
            #[inline]
            pub const fn wrapping_add(self, rhs: Self) -> Self {
                Self(self.0.wrapping_add(rhs.0))
            }

            /// Adds, also reporting whether the result wrapped.
            #[must_use]
            #[inline]
            pub const fn overflowing_add(self, rhs: Self) -> (Self, bool) {
                let (bits, overflowed) = self.0.overflowing_add(rhs.0);
                (Self(bits), overflowed)
            }

            /// Subtracts, returning `None` on overflow.
            #[must_use]
            #[inline]
            pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
                match self.0.checked_sub(rhs.0) {
                    Some(bits) => Some(Self(bits)),
                    None => None,
                }
            }

            /// Subtracts, clamping to [`MIN`](Self::MIN) or [`MAX`](Self::MAX).
            #[must_use]
            #[inline]
            pub const fn saturating_sub(self, rhs: Self) -> Self {
                Self(self.0.saturating_sub(rhs.0))
            }

            /// Subtracts, wrapping around on overflow.
            #[must_use]
            #[inline]
            pub const fn wrapping_sub(self, rhs: Self) -> Self {
                Self(self.0.wrapping_sub(rhs.0))
            }

            /// Subtracts, also reporting whether the result wrapped.
            #[must_use]
            #[inline]
            pub const fn overflowing_sub(self, rhs: Self) -> (Self, bool) {
                let (bits, overflowed) = self.0.overflowing_sub(rhs.0);
                (Self(bits), overflowed)
            }

            /// The product in `wide` bits, rounded to this type's resolution.
            #[inline]
            const fn mul_raw(self, rhs: Self) -> $wide {
                let product = (self.0 as $wide) * (rhs.0 as $wide);
                let half = 1 << ($frac - 1);
                if product >= 0 {
                    (product + half) >> $frac
                } else {
                    -((-product + half) >> $frac)
                }
            }

            /// The quotient in `wide` bits, rounded to this type's resolution.
            ///
            /// `rhs` must be non-zero.
            #[inline]
            const fn div_raw(self, rhs: Self) -> $wide {
                let numerator = (self.0 as $wide) << $frac;
                let (numerator, denominator) = if rhs.0 < 0 {
                    (-numerator, -(rhs.0 as $wide))
                } else {
                    (numerator, rhs.0 as $wide)
                };
                if numerator >= 0 {
                    (2 * numerator + denominator) / (2 * denominator)
                } else {
                    -((-2 * numerator + denominator) / (2 * denominator))
                }
            }

            /// Clamps a wide bit pattern into range.
            #[inline]
            const fn saturate(wide: $wide) -> Self {
                if wide > <$repr>::MAX as $wide {
                    Self::MAX
                } else if wide < <$repr>::MIN as $wide {
                    Self::MIN
                } else {
                    Self(wide as $repr)
                }
            }

            /// Checks that a wide bit pattern is in range.
            #[inline]
            const fn check(wide: $wide) -> Option<Self> {
                if wide > <$repr>::MAX as $wide || wide < <$repr>::MIN as $wide {
                    None
                } else {
                    Some(Self(wide as $repr))
                }
            }

            /// Multiplies, returning `None` on overflow.
            ///
            /// The product is computed at full width and rounded once, so the
            /// result is the representable value nearest the true product.
            #[must_use]
            #[inline]
            pub const fn checked_mul(self, rhs: Self) -> Option<Self> {
                Self::check(self.mul_raw(rhs))
            }

            /// Multiplies, clamping to [`MIN`](Self::MIN) or [`MAX`](Self::MAX).
            #[must_use]
            #[inline]
            pub const fn saturating_mul(self, rhs: Self) -> Self {
                Self::saturate(self.mul_raw(rhs))
            }

            /// Multiplies, wrapping around on overflow.
            #[must_use]
            #[inline]
            pub const fn wrapping_mul(self, rhs: Self) -> Self {
                Self(self.mul_raw(rhs) as $repr)
            }

            /// Multiplies, also reporting whether the result wrapped.
            #[must_use]
            #[inline]
            pub const fn overflowing_mul(self, rhs: Self) -> (Self, bool) {
                let wide = self.mul_raw(rhs);
                let overflowed = wide > <$repr>::MAX as $wide || wide < <$repr>::MIN as $wide;
                (Self(wide as $repr), overflowed)
            }

            /// Divides, returning `None` on overflow or division by zero.
            #[must_use]
            #[inline]
            pub const fn checked_div(self, rhs: Self) -> Option<Self> {
                if rhs.0 == 0 {
                    return None;
                }
                Self::check(self.div_raw(rhs))
            }

            /// Divides, clamping to [`MIN`](Self::MIN) or [`MAX`](Self::MAX).
            ///
            /// Division by zero saturates in the direction of the numerator's
            /// sign, and `0 / 0` is [`ZERO`](Self::ZERO). The operation is
            /// total, which is what lets `/` exist at all without a panic.
            #[must_use]
            #[inline]
            pub const fn saturating_div(self, rhs: Self) -> Self {
                if rhs.0 == 0 {
                    return if self.0 > 0 {
                        Self::MAX
                    } else if self.0 < 0 {
                        Self::MIN
                    } else {
                        Self::ZERO
                    };
                }
                Self::saturate(self.div_raw(rhs))
            }

            /// The remainder, or `None` if `rhs` is zero.
            ///
            /// Exact: the remainder of two multiples of `DELTA` is itself a
            /// multiple of `DELTA`.
            #[must_use]
            #[inline]
            pub const fn checked_rem(self, rhs: Self) -> Option<Self> {
                if rhs.0 == 0 {
                    None
                } else {
                    Some(Self(self.0.wrapping_rem(rhs.0)))
                }
            }

            /// The remainder, or [`ZERO`](Self::ZERO) if `rhs` is zero.
            #[must_use]
            #[inline]
            pub const fn saturating_rem(self, rhs: Self) -> Self {
                if rhs.0 == 0 {
                    Self::ZERO
                } else {
                    Self(self.0.wrapping_rem(rhs.0))
                }
            }

            /// Negates, returning `None` if the result is out of range.
            #[must_use]
            #[inline]
            pub const fn checked_neg(self) -> Option<Self> {
                match self.0.checked_neg() {
                    Some(bits) => Some(Self(bits)),
                    None => None,
                }
            }

            /// Negates, clamping to [`MAX`](Self::MAX).
            ///
            /// The range is asymmetric, so negating [`MIN`](Self::MIN) has to
            /// clamp.
            #[must_use]
            #[inline]
            pub const fn saturating_neg(self) -> Self {
                Self(self.0.saturating_neg())
            }

            /// Negates, wrapping around on overflow.
            #[must_use]
            #[inline]
            pub const fn wrapping_neg(self) -> Self {
                Self(self.0.wrapping_neg())
            }

            /// The absolute value, clamping to [`MAX`](Self::MAX).
            #[must_use]
            #[inline]
            pub const fn abs(self) -> Self {
                Self(self.0.saturating_abs())
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
            /// Computed as an integer square root of the bits scaled up by
            /// `2^FRAC_BITS`, so no floating point is involved. Negative inputs
            /// return [`ZERO`](Self::ZERO) rather than panicking, and results
            /// above [`MAX`](Self::MAX) saturate.
            #[must_use]
            #[inline]
            pub const fn sqrt(self) -> Self {
                if self.0 <= 0 {
                    return Self::ZERO;
                }
                let scaled = (self.0 as $uwide) << $frac;
                let root = scaled.isqrt();
                // Round up when the true root is past the halfway point, which
                // happens exactly when the remainder exceeds the root.
                let rounded = if scaled - root * root > root { root + 1 } else { root };
                Self::saturate(rounded as $wide)
            }

            /// The square root, or `None` for a negative input.
            #[must_use]
            #[inline]
            pub const fn checked_sqrt(self) -> Option<Self> {
                if self.0 < 0 { None } else { Some(self.sqrt()) }
            }

            /// Mask selecting the fractional bits.
            const FRAC_MASK: $wide = (1 << $frac) - 1;

            /// The largest integer not greater than this value, saturating.
            ///
            /// Masking off the fractional bits of a two's-complement integer
            /// rounds toward negative infinity, which is exactly what floor is.
            #[must_use]
            #[inline]
            pub const fn floor(self) -> Self {
                Self::saturate((self.0 as $wide) & !Self::FRAC_MASK)
            }

            /// The smallest integer not less than this value, saturating.
            #[must_use]
            #[inline]
            pub const fn ceil(self) -> Self {
                Self::saturate(((self.0 as $wide) + Self::FRAC_MASK) & !Self::FRAC_MASK)
            }

            /// The nearest integer, with halfway cases rounding away from zero.
            ///
            /// Matches `f64::round`, saturating rather than growing out of range.
            #[must_use]
            #[inline]
            pub const fn round(self) -> Self {
                let bits = self.0 as $wide;
                let half = 1 << ($frac - 1);
                Self::saturate(if bits >= 0 {
                    (bits + half) & !Self::FRAC_MASK
                } else {
                    -((-bits + half) & !Self::FRAC_MASK)
                })
            }

            /// The integer part, rounding toward zero.
            #[must_use]
            #[inline]
            pub const fn trunc(self) -> Self {
                let bits = self.0 as $wide;
                Self::saturate(if bits >= 0 {
                    bits & !Self::FRAC_MASK
                } else {
                    -((-bits) & !Self::FRAC_MASK)
                })
            }

            /// The fractional part, `self - self.trunc()`.
            ///
            /// Carries the sign of `self`, as `f64::fract` does. Always exact.
            #[must_use]
            #[inline]
            pub const fn fract(self) -> Self {
                Self((self.0 as $wide % (Self::FRAC_MASK + 1)) as $repr)
            }

            /// The reciprocal, clamping to [`MIN`](Self::MIN) or
            /// [`MAX`](Self::MAX).
            ///
            /// The reciprocal of zero saturates to [`MAX`](Self::MAX), and for
            /// [`I0F8`] — whose values are all under `0.5` in magnitude — the
            /// result always saturates.
            #[must_use]
            #[inline]
            pub const fn recip(self) -> Self {
                if self.0 == 0 {
                    return Self::MAX;
                }
                Self::saturate(Self::recip_raw(self.0 as $wide))
            }

            /// The reciprocal, or `None` if zero or out of range.
            #[must_use]
            #[inline]
            pub const fn checked_recip(self) -> Option<Self> {
                if self.0 == 0 {
                    return None;
                }
                Self::check(Self::recip_raw(self.0 as $wide))
            }

            /// One divided by a non-zero bit pattern, in `wide` bits.
            #[inline]
            const fn recip_raw(bits: $wide) -> $wide {
                // One in this type's bits, shifted up by the same scale again, so
                // the quotient lands back at the type's own resolution.
                let numerator = (1 as $wide) << (2 * $frac);
                let (numerator, denominator) =
                    if bits < 0 { (-numerator, -bits) } else { (numerator, bits) };
                if numerator >= 0 {
                    (2 * numerator + denominator) / (2 * denominator)
                } else {
                    -((-2 * numerator + denominator) / (2 * denominator))
                }
            }

            /// Computes `self * factor + addend` with a single rounding.
            ///
            /// The product is kept at full width and the addend folded in before
            /// rounding, so this is more accurate than multiplying and then adding
            /// — the same reason `f64::mul_add` exists. Saturates.
            #[must_use]
            #[inline]
            pub const fn mul_add(self, factor: Self, addend: Self) -> Self {
                let product = (self.0 as $wide) * (factor.0 as $wide);
                let scaled_addend = (addend.0 as $wide) << $frac;
                let sum = product + scaled_addend;
                let half = 1 << ($frac - 1);
                Self::saturate(if sum >= 0 {
                    (sum + half) >> $frac
                } else {
                    -((-sum + half) >> $frac)
                })
            }

            /// The length of the hypotenuse, `sqrt(self^2 + other^2)`.
            ///
            /// Computed by integer square root of the exact sum of squares, so no
            /// intermediate overflows the way a naive `(a*a + b*b).sqrt()` would.
            /// Saturates at [`MAX`](Self::MAX).
            #[must_use]
            #[inline]
            pub const fn hypot(self, other: Self) -> Self {
                let a = self.0.unsigned_abs() as $uwide;
                let b = other.0.unsigned_abs() as $uwide;
                let sum = a * a + b * b;
                let root = sum.isqrt();
                let rounded = if sum - root * root > root { root + 1 } else { root };
                Self::saturate(rounded as $wide)
            }

            #[doc = concat!("Linearly interpolates toward `to`, using a [`", stringify!($factor), "`] weight.")]
            ///
            /// Exact at both ends: a weight of
            #[doc = concat!("[`", stringify!($factor), "::ZERO`] returns `self` and [`", stringify!($factor), "::ONE`] returns `to`,")]
            /// and every intermediate result lies between the two endpoints, so
            /// this never overflows.
            #[must_use]
            #[inline]
            pub const fn lerp(self, to: Self, weight: $factor) -> Self {
                let delta = to.0 as i128 - self.0 as i128;
                let numerator = delta * weight.to_bits() as i128;
                let denominator = $factor::MAX.to_bits() as i128;
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
        impl_binop!($name, Mul::mul, MulAssign::mul_assign, saturating_mul);
        impl_binop!($name, Div::div, DivAssign::div_assign, saturating_div);
        impl_binop!($name, Rem::rem, RemAssign::rem_assign, saturating_rem);
        impl_neg!($name, saturating_neg);
        impl_num_traits_shared!($name);
        impl_num_traits_arith!($name);
        impl_num_traits_wrapping!($name);
    };
}

/// Rounds an `f64` half away from zero, keeping `NaN` at zero.
///
/// The caller casts the result to an integer, where Rust's saturating
/// float-to-int conversion supplies the clamping and the `NaN` behavior.
const fn round_f64(scaled: f64) -> f64 {
    if scaled >= 0.0 {
        scaled + 0.5
    } else {
        scaled - 0.5
    }
}

define_fixed_point! {
    /// A signed fixed-point number with no integer bits and 8 fractional bits.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i8` |
    /// | Range | `-0.5 ..= 0.49609375` |
    /// | Resolution | `1/256`, or `0.00390625` |
    ///
    /// `1.0` falls outside the range, so this is the one type in the crate with
    /// no `ONE` and no `num_traits::One` implementation. Products of two
    /// `I0F8` values are always in range; sums, quotients, and square roots
    /// saturate.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::I0F8;
    ///
    /// let quarter = I0F8::from_f64(0.25);
    /// assert_eq!(quarter.to_bits(), 64);
    /// assert_eq!((quarter * quarter).to_f64(), 0.0625);
    ///
    /// // 0.5 is one step past the top of the range, so sums saturate early.
    /// assert_eq!(quarter + quarter, I0F8::MAX);
    /// assert_eq!(quarter.checked_add(quarter), None);
    /// ```
    I0F8(i8) {
        wide: i32,
        uwide: u32,
        frac: 8,
        factor: Factor8,
    }
}

define_fixed_point! {
    /// A signed fixed-point number with 8 integer bits and 8 fractional bits.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i16` |
    /// | Range | `-128.0 ..= 127.99609375` |
    /// | Resolution | `1/256`, or `0.00390625` |
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::I8F8;
    ///
    /// let a = I8F8::from_f64(1.5);
    /// let b = I8F8::from_f64(-0.25);
    /// assert_eq!((a * b).to_f64(), -0.375);
    /// assert_eq!((a / b).to_f64(), -6.0);
    /// assert_eq!(I8F8::from_f64(2.25).sqrt().to_f64(), 1.5);
    ///
    /// // Every value round-trips through f32 and f64 unchanged.
    /// assert_eq!(I8F8::from_f32(a.to_f32()), a);
    ///
    /// // Overflow saturates by default and is detectable on demand.
    /// assert_eq!(I8F8::MAX + I8F8::ONE, I8F8::MAX);
    /// assert_eq!(I8F8::MAX.checked_add(I8F8::ONE), None);
    /// assert_eq!(I8F8::MAX.overflowing_add(I8F8::ONE).1, true);
    /// ```
    I8F8(i16) {
        wide: i32,
        uwide: u32,
        frac: 8,
        factor: Factor16,
    }
}

define_fixed_point! {
    /// A signed fixed-point number with 24 integer bits and 8 fractional bits.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i32` |
    /// | Range | `-8388608.0 ..= 8388607.99609375` |
    /// | Resolution | `1/256`, or `0.00390625` |
    ///
    /// The workhorse of the family: wide enough for world-space positions at
    /// millimetre-ish resolution while still hashing as a single `i32`.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::{Factor32, I24F8};
    ///
    /// let start = I24F8::from_f64(-1000.5);
    /// let end = I24F8::from_f64(1000.5);
    /// assert_eq!(start.lerp(end, Factor32::ZERO), start);
    /// assert_eq!(start.lerp(end, Factor32::ONE), end);
    /// assert_eq!(start.lerp(end, Factor32::from_f64(0.5)), I24F8::ZERO);
    ///
    /// assert_eq!(I24F8::from_f64(2.0).sqrt().to_f64(), 1.4140625);
    ///
    /// // f64 is lossless; f32 has too few mantissa bits for the full range.
    /// let wide = I24F8::from_f64(8_000_000.5);
    /// assert_eq!(I24F8::from_f64(wide.to_f64()), wide);
    /// ```
    I24F8(i32) {
        wide: i64,
        uwide: u64,
        frac: 8,
        factor: Factor32,
    }
}

impl_one!(I8F8, 256);
impl_one!(I24F8, 256);
