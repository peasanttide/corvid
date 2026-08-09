//! The generator for the trigonometry on a wrapping angle.

macro_rules! define_angle_trig {
    (
        $name:ident, $repr:ty, $srepr:ty, $phase_shift:expr,
        $signed:ident($signed_repr:ty), $factor:ident, $pitch:ident
    ) => {
        impl $name {
            #[doc = concat!("The sine, as a [`", stringify!($signed), "`].")]
            ///
            /// Correctly rounded: the same bit pattern that rounding the true
            /// sine to this output type would produce, verified over the whole
            /// domain at every width. Exact at multiples of a quarter turn.
            #[must_use]
            #[inline]
            pub const fn sin(self) -> $signed {
                let scale = $signed::MAX.to_bits() as i64;
                $signed::from_bits(trig::sin_snorm(self.phase(), scale) as $signed_repr)
            }

            #[doc = concat!("The cosine, as a [`", stringify!($signed), "`].")]
            ///
            /// Correctly rounded: the same bit pattern that rounding the true
            /// cosine to this output type would produce, verified over the whole
            /// domain at every width. Exact at multiples of a quarter turn.
            #[must_use]
            #[inline]
            pub const fn cos(self) -> $signed {
                let scale = $signed::MAX.to_bits() as i64;
                $signed::from_bits(trig::cos_snorm(self.phase(), scale) as $signed_repr)
            }

            /// The sine and cosine together.
            ///
            /// Identical to calling [`sin`](Self::sin) and [`cos`](Self::cos),
            /// and clearer at the call site when both are wanted.
            #[must_use]
            #[inline]
            pub const fn sin_cos(self) -> ($signed, $signed) {
                (self.sin(), self.cos())
            }

            /// The sine, approximately.
            ///
            /// Worst-case error is `1.2e-3` -- measured at `1.1111e-3`, over
            /// every one of the 2^32 phases -- from a parabola corrected by a
            /// second parabola in its own output. Exact at multiples of a
            /// quarter turn, and exactly odd in the phase. Within a bit for
            /// [`Signed8`] outputs; about 36 bits coarse for [`Signed16`].
            ///
            /// Computed entirely in 32-bit integer arithmetic, using only
            /// operations a shader has, so the algorithm transcribes directly
            /// into `WGSL`. That is a deliberate constraint rather than an
            /// accident of the implementation: see `trig::sin_fast_q30`.
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
                self.wrapping_add(Self::QUARTER_TURN).sin_fast()
            }

            /// The sine and cosine together, approximately.
            #[must_use]
            #[inline]
            pub const fn sin_cos_fast(self) -> ($signed, $signed) {
                (self.sin_fast(), self.cos_fast())
            }

            /// The tangent, as an [`I24F8`].
            ///
            /// Unbounded, so the result is a fixed-point type rather than a
            /// normalized one: it saturates at
            /// [`I24F8::MAX`]/[`I24F8::MIN`] at the poles, where the cosine is
            /// exactly zero. Resolution is `1/256`, which is coarse near the
            /// poles where the tangent is steep.
            #[must_use]
            #[inline]
            pub const fn tan(self) -> I24F8 {
                I24F8::from_bits(trig::tan_i24f8(self.phase()))
            }

            /// The angle of the vector `(x, y)`, measured counterclockwise from
            /// the positive `x` axis.
            ///
            /// Scale invariant: only the ratio matters, so any consistent units
            /// work -- raw fixed-point bits, integer grid coordinates, pixel
            /// offsets. Both arguments must be in the *same* units. `(0, 0)`
            /// returns [`ZERO`](Self::ZERO).
            ///
            /// Computed by CORDIC, which needs only shifts and adds -- no
            /// division -- and lands within one bit of this type.
            #[must_use]
            #[inline]
            pub const fn atan2(y: i64, x: i64) -> Self {
                Self(trig::atan2_bits(y, x, <$repr>::BITS) as $repr)
            }

            #[doc = concat!("The arccosine of a [`", stringify!($signed), "`], from zero to a half turn.")]
            ///
            /// The inverse of [`cos`](Self::cos) over the half turn where the
            /// cosine is one-to-one. Computed as `pi/2 - asin(value)`, so it
            #[doc = concat!(
                "inherits [`asin`](crate::", stringify!($pitch), "::asin)'s accuracy, and is",
            )]
            /// exact at `-1`, `0`, and `1`.
            ///
            #[doc = concat!(
                "The arcsine's counterpart lives on [`", stringify!($pitch),
                "`](crate::", stringify!($pitch), ") instead, whose range is exactly the",
            )]
            /// arcsine's.
            #[must_use]
            #[inline]
            pub const fn acos(value: $signed) -> Self {
                const SCALE: i64 = $signed::MAX.to_bits() as i64;
                const RECIPROCAL: i128 = trig::snorm_reciprocal(SCALE);
                let arcsine = trig::asin_bits(
                    value.canonicalize().to_bits() as i64,
                    SCALE,
                    RECIPROCAL,
                    <$repr>::BITS,
                );
                Self::QUARTER_TURN.wrapping_sub(Self(arcsine as $repr))
            }

            /// The angle of the vector `(x, y)`, approximately.
            ///
            /// Worst-case error is `4.4e-3` radians. See
            /// [`atan2`](Self::atan2) for the conventions.
            ///
            /// The coordinates are `i32` where [`atan2`](Self::atan2) takes
            /// `i64`, because like [`sin_fast`](Self::sin_fast) this is computed
            /// entirely in 32-bit integer arithmetic and transcribes directly
            /// into a shader. Scale invariance is unchanged, so a caller holding
            /// wider values can shift both coordinates down to fit.
            #[must_use]
            #[inline]
            pub const fn atan2_fast(y: i32, x: i32) -> Self {
                Self(trig::atan2_fast_bits(y, x, <$repr>::BITS) as $repr)
            }        }
    };
}

pub(super) use define_angle_trig;
