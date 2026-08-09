//! The generator for the arithmetic that needs the widened intermediate:
//! multiplication, division, the remainder, negation and the two roots.

macro_rules! define_fixed_point_math {
    ($name:ident, $repr:ty, $wide:ty, $uwide:ty, $frac:expr, $factor:ident) => {
        impl $name {
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
                let rounded = if scaled - root * root > root {
                    root + 1
                } else {
                    root
                };
                Self::saturate(rounded as $wide)
            }

            /// The square root, or `None` for a negative input.
            #[must_use]
            #[inline]
            pub const fn checked_sqrt(self) -> Option<Self> {
                if self.0 < 0 { None } else { Some(self.sqrt()) }
            }

            /// The reciprocal square root, correctly rounded.
            ///
            /// One rounding, from a full-width intermediate -- unlike
            /// `x.sqrt().recip()`, which rounds at the square root and again at
            /// the reciprocal, and whose intermediate can saturate on its own
            /// for small `x`. There is no division and no `isqrt` loop: the
            /// estimate is seeded from `leading_zeros` and refined by
            /// Newton-Raphson, then an exact integer comparison picks between
            /// the two neighbouring results.
            ///
            /// Zero and negatives saturate to [`MAX`](Self::MAX), matching how
            /// [`recip`](Self::recip) treats zero. Results above
            /// [`MAX`](Self::MAX) saturate too, which for [`I0F8`] -- whose
            /// values are all under `0.5` -- is every input.
            #[must_use]
            #[inline]
            pub const fn rsqrt(self) -> Self {
                if self.0 <= 0 {
                    return Self::MAX;
                }
                Self::saturate(super::rsqrt::rsqrt_bits(self.0 as u64, $frac) as $wide)
            }

            /// The reciprocal square root, or `None` for zero, a negative, or a
            /// result past [`MAX`](Self::MAX).
            #[must_use]
            #[inline]
            pub const fn checked_rsqrt(self) -> Option<Self> {
                if self.0 <= 0 {
                    return None;
                }
                Self::check(super::rsqrt::rsqrt_bits(self.0 as u64, $frac) as $wide)
            }
        }
    };
}

pub(super) use define_fixed_point_math;
