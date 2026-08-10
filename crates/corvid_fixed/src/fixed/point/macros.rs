//! The generator for the six fixed-point types: the newtype, the `f64`
//! conversions and the additive arithmetic.
//!
//! Multiplication and the roots are in
//! [`define_fixed_point_math`](super::math::define_fixed_point_math) and the
//! roundings in
//! [`define_fixed_point_round`](super::round::define_fixed_point_round),
//! because a file stays under 400 lines.

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
                let low = <$repr>::MIN as f64;
                // `low - 0.5` is exact only while the repr is narrower than
                // `f64`'s mantissa. At `i64` it collapses back onto `low`, and
                // the strict `>` would then reject `MIN` itself -- an exactly
                // representable value. `|| scaled >= low` restores that one
                // endpoint without loosening any other width: where
                // `low - 0.5` *is* representable the second test is implied by
                // the first, and at `i64` the next `f64` below `-2^63` is a
                // full 2048 steps away, so nothing else slips through.
                //
                // The upper bound needs no such care: every `f64` strictly
                // below `MAX as f64 + 0.5` also satisfies the exact bound.
                //
                // A NaN fails every comparison, so it lands in `None`.
                if (scaled > low - 0.5 || scaled >= low)
                    && scaled < <$repr>::MAX as f64 + 0.5
                {
                    Some(Self(round_f64(scaled) as $repr))
                } else {
                    None
                }
            }

            /// The value as an `f64`, correctly rounded.
            ///
            /// Lossless for every type in this family except [`I48F16`], whose
            /// 63 magnitude bits exceed `f64`'s 53-bit mantissa. See that
            /// type's own documentation.
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

            #[doc = concat!("A wide bit pattern as a [`", stringify!($name), "`], clamping.")]
            ///
            /// The narrowing every accumulator in this workspace ends with. A
            /// caller that has widened to hold an intermediate comes back
            /// through here, and a value past the range clamps rather than
            /// wrapping -- an offset that wrapped would come back pointing the
            /// other way, which is worse than one that is merely far.
            #[must_use]
            #[inline]
            pub const fn saturating_from_bits(wide: $wide) -> Self {
                Self::saturate(wide)
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
pub(super) use define_fixed_point;
