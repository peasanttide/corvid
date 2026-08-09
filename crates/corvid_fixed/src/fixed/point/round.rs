//! The generator for the roundings, the reciprocal and the three composed
//! operations built on them.

macro_rules! define_fixed_point_round {
    ($name:ident, $repr:ty, $wide:ty, $uwide:ty, $frac:expr, $factor:ident) => {
        impl $name {
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
            /// [`I0F8`] -- whose values are all under `0.5` in magnitude -- the
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
            /// -- the same reason `f64::mul_add` exists. Saturates.
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
            }        }
    };
}

pub(super) use define_fixed_point_round;
