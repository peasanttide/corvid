//! Rounding: the generator for the per-type roundings, and the three rules the
//! rest of the tier shares.
//!
//! Every one of these rounds half away from zero, which is the one decision
//! behind them all -- the rest of the crate rounds that way, and a helper that
//! disagreed would put the disagreement somewhere nobody would look for it.
//! [`divide`] is the rule at the width a scale conversion works in and
//! [`divide_wide`] is the same rule where the intermediates pass an `i64`;
//! [`round_f64`] is it at the boundary with a float.

/// Rounds an `f64` half away from zero, keeping `NaN` at zero.
///
/// The caller casts the result to an integer, where Rust's saturating
/// float-to-int conversion supplies the clamping and the `NaN` behavior.
pub(super) const fn round_f64(scaled: f64) -> f64 {
    if scaled >= 0.0 {
        scaled + 0.5
    } else {
        scaled - 0.5
    }
}

/// A quotient, rounded to nearest with halves away from zero.
///
/// Rust's integer division truncates toward zero, which turns every sub-unit
/// shortfall into a whole step in the last place -- systematic, in the same
/// direction every time, and enough to put a ray's hit under the surface it was
/// cast at. The caller has already rejected a zero denominator.
#[inline]
pub(super) const fn divide(numerator: i64, denominator: i64) -> i64 {
    // `unsigned_abs` rather than `abs`, which overflows on `i64::MIN` -- a
    // value no call site reaches, and a panic the workspace forbids being one
    // branch away from is not worth the shorter spelling.
    let half = (denominator.unsigned_abs() / 2) as i64;
    let bump = if numerator < 0 { -half } else { half };
    (numerator + bump) / denominator
}

/// The same rounding at the width the cube root and the Q30 matrices need.
///
/// [`divide`] is the one every scale conversion goes through; this is the twin
/// for the two places whose intermediates pass an `i64` -- a Q30 value cubed
/// reaches 2^93, and three Q60 products summed reach past 2^63 -- so that the
/// rounding is the same rule at both widths rather than two spellings of it.
#[inline]
pub(super) const fn divide_wide(numerator: i128, denominator: i128) -> i128 {
    let half = (denominator.unsigned_abs() / 2).cast_signed();
    let bump = if numerator < 0 { -half } else { half };
    (numerator + bump) / denominator
}

/// A 128-bit intermediate brought back to the width `saturate` takes, clamping.
///
/// The step before a saturating narrowing rather than a substitute for it: this
/// gets the value into an `i64` without wrapping, and `saturate` then clamps it
/// to the type's own range.
#[inline]
#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is clamped to i64's range on the two branches above the cast, which is what makes the narrowing exact"
)]
pub(super) const fn narrow_i64(value: i128) -> i64 {
    if value > i64::MAX as i128 {
        i64::MAX
    } else if value < i64::MIN as i128 {
        i64::MIN
    } else {
        value as i64
    }
}

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

            /// The whole part as an `i32`, and the **non-negative** remainder
            /// left above it.
            ///
            /// The two reconstruct the value -- `whole + remainder == self` --
            /// which is the property that makes this worth having as one
            /// operation rather than two: a caller splitting a large coordinate
            /// into an exact integer it can subtract and a small remainder it
            /// can afford to convert needs the pair to still add up.
            ///
            /// # Not [`trunc`](Self::trunc) and [`fract`](Self::fract)
            ///
            /// Those two round toward zero and give the remainder the sign of
            /// the input, so a value of `-0.25` splits as `(0, -0.25)`. This
            /// one floors, so the same value splits as `(-1, 0.75)` and the
            /// remainder is in `[0, 1)` on both sides of zero. A caller that
            /// hands the remainder to something unsigned -- or that just wants
            /// one case instead of two -- wants this one.
            ///
            /// # When the whole part does not fit
            ///
            #[doc = concat!("An [`", stringify!($name), "`] whose integer part is outside an `i32`")]
            /// saturates it, and the remainder absorbs the difference rather
            /// than being discarded: the sum is still the original value, so
            /// nothing is silently lost, but the remainder is no longer under
            /// one. Only [`I48F16`] can reach that at all; every other type
            /// here has an integer part an `i32` holds exactly.
            #[must_use]
            #[inline]
            pub const fn split_floor(self) -> (i32, Self) {
                let bits = self.0 as $wide;
                // An arithmetic shift rounds toward negative infinity, which is
                // what floor is.
                let whole = bits >> $frac;
                let whole = if whole > i32::MAX as $wide {
                    i32::MAX
                } else if whole < i32::MIN as $wide {
                    i32::MIN
                } else {
                    whole as i32
                };
                (whole, Self::saturate(bits - ((whole as $wide) << $frac)))
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
