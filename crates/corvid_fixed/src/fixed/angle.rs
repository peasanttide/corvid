//! Wrapping angles, and the trigonometry defined on them.
//!
//! An angle is a binary angle measurement: the storage type spans exactly one
//! turn, so a value `v` denotes `v / 2^BITS` turns and arithmetic wraps at the
//! full turn for free. There is no invalid angle, no normalization step, and no
//! accumulated drift from repeatedly adding to a heading.
//!
//! Wrapping is the only sensible overflow behavior on a circle, so these are the
//! one family with no `checked_` or `saturating_` operations. `+` and `-` wrap.
//!
//! # Trigonometry
//!
//! Trigonometry lives here rather than on the numeric families because an angle
//! is the only type that knows its own units. Two tiers are available:
//!
//! - [`sin`](Angle16::sin), [`cos`](Angle16::cos),
//!   [`sin_cos`](Angle16::sin_cos), [`tan`](Angle16::tan), and
//!   [`atan2`](Angle16::atan2) are the accurate tier. Sine and cosine are
//!   *correctly rounded*: the result is the same bit pattern you would get by
//!   rounding the true value to the output type, at every width. `tests/trig.rs`
//!   proves it for [`Angle8`] and [`Angle16`] by walking all 256 and all 65536
//!   inputs against `f64`. [`Angle32`] is finer than `f64` can referee, so it is
//!   held to a table of values computed in 80-digit arithmetic, plus a sweep of
//!   all 2^32 phases against the extended-precision path the implementation
//!   falls back to near a rounding boundary. That fallback costs [`Angle32`]
//!   about a tenth of its time and the narrower types nothing, since Q60 already
//!   rounds every input they have correctly.
//! - [`sin_fast`](Angle16::sin_fast), [`cos_fast`](Angle16::cos_fast), and
//!   [`atan2_fast`](Angle16::atan2_fast) trade accuracy for speed: worst-case
//!   error is `1.2e-3` for sine and `4.4e-3` radians for arctangent.
//!   Exact enough for [`Angle8`]/[`Signed8`], coarse for the
//!   wider types. They are also 32-bit clean — no 64-bit intermediate, and no
//!   operation `WGSL` lacks — so they transcribe directly into a shader, which
//!   is why [`atan2_fast`](Angle16::atan2_fast) takes `i32` coordinates where
//!   [`atan2`](Angle16::atan2) takes `i64`.
//!
//! Both tiers are `const` and use only integer arithmetic, so results are
//! bit-identical on every target — a requirement for the deterministic
//! simulation this crate exists to serve.

use super::factor::{Factor8, Factor16, Factor32};
use super::macros::{
    define_newtype, impl_binop, impl_neg, impl_num_traits_shared, impl_num_traits_wrapping,
    impl_shared,
};
use super::point::I24F8;
use super::signed::{Signed8, Signed16, Signed32};
use crate::trig;

/// Generates a wrapping angle type.
///
/// `phase_shift` widens the stored bits to the `u32` phase that [`trig`] works
/// in; `signed` is the trigonometric output type of matching width, and `pitch`
/// the clamping angle that shares its scale.
macro_rules! define_angle {
    (
        $(#[$attr:meta])*
        $name:ident($repr:ty) {
            signed_repr: $srepr:ty,
            phase_shift: $phase_shift:expr,
            signed: $signed:ident($signed_repr:ty),
            factor: $factor:ident,
            pitch: $pitch:ident,
        }
    ) => {
        define_newtype! {
            $(#[$attr])*
            $name($repr)
        }

        impl $name {
            /// The smallest bit pattern, which is zero.
            ///
            /// The circle has no least element; this exists so that
            /// `num_traits::Bounded` and range-style code have something to
            /// name. Ordering on angles is ordering on the stored phase,
            /// measured counterclockwise from zero.
            pub const MIN: Self = Self(0);

            /// The largest bit pattern, one [`DELTA`](Self::DELTA) short of a
            /// full turn.
            pub const MAX: Self = Self(<$repr>::MAX);

            /// The difference between adjacent representable angles.
            pub const DELTA: Self = Self(1);

            /// A quarter turn: 90 degrees, or `pi/2` radians.
            pub const QUARTER_TURN: Self = Self(1 << (<$repr>::BITS - 2));

            /// A half turn: 180 degrees, or `pi` radians.
            pub const HALF_TURN: Self = Self(1 << (<$repr>::BITS - 1));

            /// Three quarters of a turn: 270 degrees, or `3*pi/2` radians.
            pub const THREE_QUARTER_TURN: Self = Self(3 << (<$repr>::BITS - 2));

            /// Bit patterns in one full turn.
            const TURN: f64 = (1u64 << <$repr>::BITS) as f64;

            /// Converts from turns, wrapping into range.
            ///
            /// Halfway cases round away from zero. Every finite input wraps,
            /// however many turns it names: `NaN` and both infinities become
            /// [`ZERO`](Self::ZERO), since a circle has no bound to saturate
            /// against.
            #[must_use]
            #[inline]
            pub const fn from_turns(turns: f64) -> Self {
                // Discard whole turns before scaling. `turns * Self::TURN`
                // alone leaves the `i64` the cast goes through once `|turns|`
                // reaches `2^(63 - BITS)` — only `2^31` turns for `Angle32` —
                // and the cast would then saturate instead of wrapping,
                // silently returning an angle near a full turn for an input
                // that should have wrapped to a quarter of one. The remainder
                // is a subtraction of a whole number of turns, so it is exact
                // and costs no accuracy. `fmod` sends both infinities to `NaN`,
                // and `NaN as i64` is `0`.
                let scaled = (turns % 1.0) * Self::TURN;
                let rounded = if scaled >= 0.0 { scaled + 0.5 } else { scaled - 0.5 };
                Self(rounded as i64 as $repr)
            }

            /// Converts from radians, wrapping into range.
            #[must_use]
            #[inline]
            pub const fn from_radians(radians: f64) -> Self {
                Self::from_turns(radians / core::f64::consts::TAU)
            }

            /// Converts from degrees, wrapping into range.
            #[must_use]
            #[inline]
            pub const fn from_degrees(degrees: f64) -> Self {
                Self::from_turns(degrees / 360.0)
            }

            /// The angle in turns, in `0.0 .. 1.0`.
            #[must_use]
            #[inline]
            pub const fn to_turns(self) -> f64 {
                self.0 as f64 / Self::TURN
            }

            /// The angle in radians, in `0.0 .. 2*pi`.
            #[must_use]
            #[inline]
            pub const fn to_radians(self) -> f64 {
                self.to_turns() * core::f64::consts::TAU
            }

            /// The angle in degrees, in `0.0 .. 360.0`.
            #[must_use]
            #[inline]
            pub const fn to_degrees(self) -> f64 {
                self.to_turns() * 360.0
            }

            /// The angle in turns, in `-0.5 .. 0.5`.
            #[must_use]
            #[inline]
            pub const fn to_signed_turns(self) -> f64 {
                self.to_signed_bits() as f64 / Self::TURN
            }

            /// The angle in radians, in `-pi .. pi`.
            ///
            /// The signed convention `atan2` and shortest-arc code expect.
            #[must_use]
            #[inline]
            pub const fn to_signed_radians(self) -> f64 {
                self.to_signed_turns() * core::f64::consts::TAU
            }

            /// The stored phase reinterpreted as a signed offset from zero,
            /// covering half a turn either way.
            #[must_use]
            #[inline]
            pub const fn to_signed_bits(self) -> $srepr {
                self.0 as $srepr
            }

            /// The `f64` value used for display and conversion: turns.
            #[must_use]
            #[inline]
            pub const fn to_f64(self) -> f64 {
                self.to_turns()
            }

            /// Converts from turns, wrapping into range.
            ///
            /// Named for consistency with the other families, where the natural
            /// `f64` reading of a value is its magnitude rather than its turns.
            #[must_use]
            #[inline]
            pub const fn from_f64(turns: f64) -> Self {
                Self::from_turns(turns)
            }

            /// Converts from turns, or returns `None` if the value needed
            /// wrapping to fit one turn.
            ///
            /// Accepts exactly what lands on a bit pattern of the *same* turn:
            /// `-0.5` of a step below zero up to half a step below a full turn.
            /// A value inside `0.0 .. 1.0` that rounds up onto the full turn —
            /// `0.999` for an [`Angle8`], whose steps are `1/256` — is
            /// therefore rejected, because the angle it produces is `ZERO`
            /// rather than anything near the input.
            ///
            /// Angles wrap, so this rejects nothing an angle cannot *hold*; it
            /// exists for `num_traits::FromPrimitive` and for callers that
            /// want to know a value needed wrapping. `NaN` returns `None`.
            #[must_use]
            #[inline]
            pub const fn checked_from_f64(turns: f64) -> Option<Self> {
                let scaled = turns * Self::TURN;
                if scaled > -0.5 && scaled < Self::TURN - 0.5 {
                    Some(Self::from_turns(turns))
                } else {
                    None
                }
            }

            /// The bit pattern used for comparison and hashing.
            #[inline]
            const fn cmp_key(self) -> $repr {
                self.0
            }

            /// The angle as a phase across the full `u32` range.
            #[inline]
            const fn phase(self) -> u32 {
                (self.0 as u32) << $phase_shift
            }

            /// Adds, wrapping around the circle.
            #[must_use]
            #[inline]
            pub const fn wrapping_add(self, rhs: Self) -> Self {
                Self(self.0.wrapping_add(rhs.0))
            }

            /// Subtracts, wrapping around the circle.
            #[must_use]
            #[inline]
            pub const fn wrapping_sub(self, rhs: Self) -> Self {
                Self(self.0.wrapping_sub(rhs.0))
            }

            /// Reflects across zero, wrapping around the circle.
            #[must_use]
            #[inline]
            pub const fn wrapping_neg(self) -> Self {
                Self(self.0.wrapping_neg())
            }

            /// The shortest arc between two angles, from zero to a half turn.
            #[must_use]
            #[inline]
            pub const fn abs_diff(self, other: Self) -> Self {
                let delta = self.0.wrapping_sub(other.0);
                if delta > Self::HALF_TURN.0 {
                    Self(delta.wrapping_neg())
                } else {
                    Self(delta)
                }
            }

            #[doc = concat!("Interpolates toward `to` along the shortest arc, using a [`", stringify!($factor), "`] weight.")]
            ///
            /// The wrapped difference between two angles, read as a signed
            /// offset, *is* the shortest arc — so interpolation takes the short
            /// way around with no special cases. When the two are exactly
            /// opposite there is no shorter way, and the tie breaks
            /// **clockwise**: a half-turn difference reads as
            /// `-2^(BITS - 1)` once taken as a signed offset, so the phase
            /// decreases. Interpolating from zero to [`HALF_TURN`](Self::HALF_TURN)
            /// passes through three quarters of a turn, not one quarter.
            ///
            /// Exact at both ends.
            #[must_use]
            #[inline]
            pub const fn lerp(self, to: Self, weight: $factor) -> Self {
                let delta = to.0.wrapping_sub(self.0) as $srepr as i128;
                let numerator = delta * weight.to_bits() as i128;
                let denominator = $factor::MAX.to_bits() as i128;
                let scaled = if numerator >= 0 {
                    (2 * numerator + denominator) / (2 * denominator)
                } else {
                    -((-2 * numerator + denominator) / (2 * denominator))
                };
                Self(self.0.wrapping_add(scaled as $repr))
            }

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
            /// Worst-case error is `1.2e-3` — measured at `1.1111e-3`, over
            /// every one of the 2^32 phases — from a parabola corrected by a
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
            /// work — raw fixed-point bits, integer grid coordinates, pixel
            /// offsets. Both arguments must be in the *same* units. `(0, 0)`
            /// returns [`ZERO`](Self::ZERO).
            ///
            /// Computed by CORDIC, which needs only shifts and adds — no
            /// division — and lands within one bit of this type.
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
            }
        }

        impl_shared!($name, $repr, " turn");
        impl_binop!($name, Add::add, AddAssign::add_assign, wrapping_add);
        impl_binop!($name, Sub::sub, SubAssign::sub_assign, wrapping_sub);
        impl_neg!($name, wrapping_neg);
        impl_num_traits_shared!($name);
        impl_num_traits_wrapping!($name);
    };
}

define_angle! {
    /// An 8-bit wrapping angle.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `u8` |
    /// | Range | one full turn |
    /// | Resolution | `1/256` turn, or 1.40625 degrees |
    ///
    /// Coarse, but a whole heading in one byte — and coarse enough that
    /// [`sin_fast`](Self::sin_fast) is already accurate to the last bit of its
    /// [`Signed8`] output.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::Angle8;
    ///
    /// assert_eq!(Angle8::from_degrees(90.0), Angle8::QUARTER_TURN);
    /// assert_eq!(Angle8::from_degrees(-90.0), Angle8::THREE_QUARTER_TURN);
    ///
    /// // A full turn wraps to zero, exactly.
    /// assert_eq!(Angle8::from_degrees(360.0), Angle8::ZERO);
    /// assert_eq!(Angle8::THREE_QUARTER_TURN + Angle8::QUARTER_TURN, Angle8::ZERO);
    /// ```
    Angle8(u8) {
        signed_repr: i8,
        phase_shift: 24,
        signed: Signed8(i8),
        factor: Factor8,
        pitch: Pitch8,
    }
}

define_angle! {
    /// A 16-bit wrapping angle.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `u16` |
    /// | Range | one full turn |
    /// | Resolution | `1/65536` turn, or about 0.0055 degrees |
    ///
    /// The default choice: finer than a rendered pixel at any plausible
    /// distance, and small enough to hash and send over a wire without a second
    /// thought.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::{Angle16, Signed16};
    ///
    /// // Trigonometry is exact at the quarter turns.
    /// assert_eq!(Angle16::ZERO.sin(), Signed16::ZERO);
    /// assert_eq!(Angle16::ZERO.cos(), Signed16::MAX);
    /// assert_eq!(Angle16::QUARTER_TURN.sin(), Signed16::MAX);
    /// assert_eq!(Angle16::HALF_TURN.cos(), Signed16::MIN);
    ///
    /// // Angles wrap instead of overflowing, so headings never need clamping.
    /// let mut heading = Angle16::from_degrees(350.0);
    /// heading += Angle16::from_degrees(20.0);
    /// assert_eq!(heading.to_degrees().round(), 10.0);
    ///
    /// // The shortest arc is the wrapped difference, read as signed.
    /// let a = Angle16::from_degrees(10.0);
    /// let b = Angle16::from_degrees(350.0);
    /// assert_eq!(a.abs_diff(b).to_degrees().round(), 20.0);
    ///
    /// // atan2 takes any consistent units.
    /// assert_eq!(Angle16::atan2(1, 1), Angle16::from_degrees(45.0));
    /// assert_eq!(Angle16::atan2(-4, 0), Angle16::THREE_QUARTER_TURN);
    ///
    /// // Everything is const-evaluable, trigonometry included.
    /// const TILT: Angle16 = Angle16::from_degrees(30.0);
    /// const SINE: Signed16 = TILT.sin();
    /// assert!((SINE.to_f64() - 0.5).abs() < 1e-4);
    /// ```
    Angle16(u16) {
        signed_repr: i16,
        phase_shift: 16,
        signed: Signed16(i16),
        factor: Factor16,
        pitch: Pitch16,
    }
}

define_angle! {
    /// A 32-bit wrapping angle.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `u32` |
    /// | Range | one full turn |
    /// | Resolution | `1/2^32` turn, or about `8.4e-8` degrees |
    ///
    /// Finer than `f32` can represent anywhere on the circle. Trigonometry costs
    /// the same as the narrower angles — the shared core computes every result
    /// at 60 fractional bits regardless — but the wider [`Signed32`] output is
    /// what makes that precision visible.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::{Angle32, Signed32};
    ///
    /// let third = Angle32::from_turns(1.0 / 3.0);
    /// assert!((third.to_degrees() - 120.0).abs() < 1e-6);
    ///
    /// // Three thirds of a turn come back to (almost) zero: the residue is the
    /// // rounding of a third of a turn, not accumulated drift.
    /// let full = third + third + third;
    /// assert!(full.abs_diff(Angle32::ZERO).to_bits() <= 2);
    ///
    /// assert_eq!(Angle32::QUARTER_TURN.cos(), Signed32::ZERO);
    /// ```
    Angle32(u32) {
        signed_repr: i32,
        phase_shift: 0,
        signed: Signed32(i32),
        factor: Factor32,
        pitch: Pitch32,
    }
}
