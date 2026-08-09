//! The generator for the three wrapping angle types: the newtype, the
//! conversions and the wrapping arithmetic.
//!
//! The trigonometry defined on the same type is in
//! [`define_angle_trig`](super::trig_impl::define_angle_trig), because a file
//! stays under 400 lines.

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
                // reaches `2^(63 - BITS)` -- only `2^31` turns for `Angle32` --
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

            /// Half the angle, as a signed offset from zero, or [`None`]
            /// past a half turn.
            ///
            /// The answer is a [`Pitch32`](crate::Pitch32) rather than another
            /// angle because halving is the one operation whose result is not
            /// a phase: a full turn halves to half a turn, and on a wrapping
            /// type that is indistinguishable from halving zero. A pitch
            /// covers half a turn either way and cannot wrap, so the two cases
            /// stay apart -- which is what a field of view or a cone half
            /// angle needs.
            ///
            /// That range is also why this is fallible. A pitch stops at a
            /// quarter turn either way, so an angle past a half turn has no
            /// half it can hold, and answering the clamped value would report
            /// a right angle for three quarters of a turn. There is no field
            /// of view or cone that wide, so the [`None`] arm is a caller
            /// having asked the wrong question rather than a case to handle.
            #[must_use]
            #[inline]
            pub const fn half(self) -> Option<$crate::Pitch32> {
                let turns = self.to_turns();
                if turns <= 0.5 {
                    Some($crate::Pitch32::from_f64(turns / 2.0))
                } else {
                    None
                }
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
            /// A value inside `0.0 .. 1.0` that rounds up onto the full turn --
            /// `0.999` for an [`Angle8`], whose steps are `1/256` -- is
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
            /// offset, *is* the shortest arc -- so interpolation takes the short
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
        }

        impl_shared!($name, $repr, " turn");
        impl_binop!($name, Add::add, AddAssign::add_assign, wrapping_add);
        impl_binop!($name, Sub::sub, SubAssign::sub_assign, wrapping_sub);
        impl_neg!($name, wrapping_neg);
        impl_num_traits_shared!($name);
        impl_num_traits_wrapping!($name);
    };
}
pub(super) use define_angle;
