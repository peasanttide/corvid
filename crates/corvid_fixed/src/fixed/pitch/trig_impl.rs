//! The generator for the trigonometry on a clamping pitch, and the
//! interpolation that shares its saturating arithmetic.

macro_rules! define_pitch_trig {
    (
        $name:ident, $repr:ty, $wide:ty, $phase_shift:expr,
        $angle:ident, $signed:ident($signed_repr:ty), $factor:ident
    ) => {
        impl $name {
            #[doc = concat!("The sine, as a [`", stringify!($signed), "`].")]
            ///
            /// Correctly rounded, and exactly `+/-1` at `+/-pi/2`. Spans the whole
            /// output range, since pitch covers a full quarter turn either way.
            #[must_use]
            #[inline]
            pub const fn sin(self) -> $signed {
                let scale = $signed::MAX.to_bits() as i64;
                $signed::from_bits(trig::sin_snorm(self.phase(), scale) as $signed_repr)
            }

            #[doc = concat!("The cosine, as a [`", stringify!($signed), "`].")]
            ///
            /// Never negative: the cosine of a quarter turn either side of zero
            /// runs from `0` up to `1` and back.
            #[must_use]
            #[inline]
            pub const fn cos(self) -> $signed {
                let scale = $signed::MAX.to_bits() as i64;
                $signed::from_bits(
                    trig::cos_snorm(self.phase(), scale) as $signed_repr
                )
            }

            /// The sine and cosine together.
            #[must_use]
            #[inline]
            pub const fn sin_cos(self) -> ($signed, $signed) {
                (self.sin(), self.cos())
            }

            /// The sine, approximately. Worst-case error is `1.2e-3`, measured
            /// at `1.1111e-3`.
            ///
            /// Computed entirely in 32-bit integer arithmetic, using only
            /// operations a shader has, so the algorithm transcribes directly
            /// into `WGSL`.
            #[must_use]
            #[inline]
            pub const fn sin_fast(self) -> $signed {
                let scale = $signed::MAX.to_bits() as i32;
                $signed::from_bits(trig::q30_to_snorm(
                    trig::sin_fast_q30(self.phase()),
                    scale,
                    <$repr>::BITS,
                ) as $signed_repr)
            }

            /// The cosine, approximately. See [`sin_fast`](Self::sin_fast).
            #[must_use]
            #[inline]
            pub const fn cos_fast(self) -> $signed {
                let scale = $signed::MAX.to_bits() as i32;
                let phase = self.phase().wrapping_add(1 << 30);
                $signed::from_bits(
                    trig::q30_to_snorm(trig::sin_fast_q30(phase), scale, <$repr>::BITS)
                        as $signed_repr,
                )
            }

            /// The tangent, as an [`I24F8`].
            ///
            /// Saturates at [`MIN`](Self::MIN) and [`MAX`](Self::MAX), where the
            /// tangent is unbounded.
            #[must_use]
            #[inline]
            pub const fn tan(self) -> I24F8 {
                I24F8::from_bits(trig::tan_i24f8(self.phase()))
            }

            #[doc = concat!("The arcsine of a [`", stringify!($signed), "`].")]
            ///
            /// The inverse of [`sin`](Self::sin), and the reason this type's range
            /// is what it is: arcsine's output is exactly a quarter turn either
            /// side of zero, so every result is representable and nothing clamps.
            /// Exact at `+/-1`, which map to `+/-pi/2`.
            ///
            /// The phase arrives already signed, so the negative half needs no
            /// wrapping reinterpretation on the way into this type's storage.
            #[must_use]
            #[inline]
            pub const fn asin(value: $signed) -> Self {
                const SCALE: i64 = $signed::MAX.to_bits() as i64;
                const RECIPROCAL: i128 = trig::snorm_reciprocal(SCALE);
                let bits: i32 = trig::asin_bits(
                    value.canonicalize().to_bits() as i64,
                    SCALE,
                    RECIPROCAL,
                    <$repr>::BITS,
                );
                Self(bits as $repr)
            }

            /// The arctangent of `y / x`, clamped to `-pi/2 ..= pi/2`.
            ///
            #[doc = concat!(
                "Scale invariant, like [`", stringify!($angle), "::atan2`], but folded",
            )]
            /// onto the right half plane: a negative `x` mirrors rather than
            /// turning past vertical. With `x` positive this is a plain
            /// arctangent, and `atan2(y, 1)` is the arctangent of `y`.
            #[must_use]
            #[inline]
            pub const fn atan2(y: i64, x: i64) -> Self {
                // Saturating rather than plain negation: `i64::MIN` has no
                // positive counterpart, and losing its last bit moves the angle
                // by nothing that a phase can represent.
                let mirrored = x.saturating_abs();
                // A non-negative `x` puts the result within a quarter turn of
                // zero, and the phase is signed, so this lands in range without
                // wrapping; `canonicalize` is belt and braces.
                let bits: i32 = trig::atan2_bits(y, mirrored, <$repr>::BITS);
                Self(bits as $repr).canonicalize()
            }

            #[doc = concat!("Interpolates toward `to`, using a [`", stringify!($factor), "`] weight.")]
            ///
            /// Exact at both ends. Unlike the wrapping angles there is no short
            /// way around, so this is a straight interpolation.
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
            }        }
    };
}

pub(super) use define_pitch_trig;
