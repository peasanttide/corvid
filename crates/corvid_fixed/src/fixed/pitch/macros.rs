//! The generator for the three clamping pitch types: the newtype, the
//! conversions, the saturating arithmetic and the angle conversions.
//!
//! The trigonometry and the interpolation defined on the same type are in
//! [`define_pitch_trig`](super::trig_impl::define_pitch_trig), because a file
//! stays under 400 lines.

/// Generates a clamping quarter-turn angle type.
///
/// `angle` is the wrapping angle of the same width and scale, and `signed` is the
/// trigonometric output type.
macro_rules! define_pitch {
    (
        $(#[$attr:meta])*
        $name:ident($repr:ty) {
            wide: $wide:ty,
            phase_shift: $phase_shift:expr,
            angle: $angle:ident,
            signed: $signed:ident($signed_repr:ty),
            factor: $factor:ident,
        }
    ) => {
        define_newtype! {
            $(#[$attr])*
            $name($repr)
        }

        impl $name {
            /// The largest value: exactly `+pi/2`, a quarter turn.
            pub const MAX: Self = Self(1 << (<$repr>::BITS - 2));

            /// The smallest value: exactly `-pi/2`.
            pub const MIN: Self = Self(-(1 << (<$repr>::BITS - 2)));

            /// The difference between adjacent representable angles.
            pub const DELTA: Self = Self(1);

            /// Bit patterns in one full turn, shared with the wrapping angle of
            /// the same width.
            const TURN: f64 = (1u64 << <$repr>::BITS) as f64;

            /// Converts from turns, clamping to a quarter turn either way.
            ///
            /// Halfway cases round away from zero and `NaN` becomes
            /// [`ZERO`](Self::ZERO), so this is total.
            #[must_use]
            #[inline]
            pub const fn from_turns(turns: f64) -> Self {
                let scaled = turns * Self::TURN;
                let rounded = if scaled >= 0.0 { scaled + 0.5 } else { scaled - 0.5 };
                // The cast saturates at the storage bounds, which is wider than
                // this type's range, so clamp explicitly afterward.
                Self(rounded as $repr).canonicalize()
            }

            /// Converts from radians, clamping to `-pi/2 ..= pi/2`.
            #[must_use]
            #[inline]
            pub const fn from_radians(radians: f64) -> Self {
                Self::from_turns(radians / core::f64::consts::TAU)
            }

            /// Converts from degrees, clamping to `-90 ..= 90`.
            #[must_use]
            #[inline]
            pub const fn from_degrees(degrees: f64) -> Self {
                Self::from_turns(degrees / 360.0)
            }

            /// The angle in turns, in `-0.25 ..= 0.25`.
            #[must_use]
            #[inline]
            pub const fn to_turns(self) -> f64 {
                self.cmp_key() as f64 / Self::TURN
            }

            /// The angle in radians, in `-pi/2 ..= pi/2`.
            #[must_use]
            #[inline]
            pub const fn to_radians(self) -> f64 {
                self.to_turns() * core::f64::consts::TAU
            }

            /// The angle in degrees, in `-90.0 ..= 90.0`.
            #[must_use]
            #[inline]
            pub const fn to_degrees(self) -> f64 {
                self.to_turns() * 360.0
            }

            /// The `f64` value used for display and conversion: turns.
            #[must_use]
            #[inline]
            pub const fn to_f64(self) -> f64 {
                self.to_turns()
            }

            /// Converts from turns, clamping. See [`from_turns`](Self::from_turns).
            #[must_use]
            #[inline]
            pub const fn from_f64(turns: f64) -> Self {
                Self::from_turns(turns)
            }

            /// Converts from turns, or returns `None` if more than half a step
            /// outside `-0.25 ..= 0.25`.
            ///
            /// The half-step tolerance is what makes this the inverse of
            /// [`to_f64`](Self::to_f64): a value that rounds to an endpoint is
            /// accepted and rounded there, and only one that would have to be
            /// clamped is rejected.
            ///
            /// `NaN` returns `None`.
            #[must_use]
            #[inline]
            pub const fn checked_from_f64(turns: f64) -> Option<Self> {
                let scaled = turns * Self::TURN;
                let limit = Self::MAX.0 as f64 + 0.5;
                if scaled > -limit && scaled < limit {
                    let rounded = if scaled >= 0.0 { scaled + 0.5 } else { scaled - 0.5 };
                    Some(Self(rounded as $repr))
                } else {
                    None
                }
            }

            /// The bit pattern used for comparison and hashing, clamped into
            /// range.
            #[inline]
            const fn cmp_key(self) -> $repr {
                if self.0 > Self::MAX.0 {
                    Self::MAX.0
                } else if self.0 < Self::MIN.0 {
                    Self::MIN.0
                } else {
                    self.0
                }
            }

            /// Clamps a bit pattern that came from outside this type's range.
            ///
            /// A no-op for every value the arithmetic here can produce; reach for
            /// it when handing raw bits to something that reads them directly.
            #[must_use]
            #[inline]
            pub const fn canonicalize(self) -> Self {
                Self(self.cmp_key())
            }

            /// Returns `true` if the stored bits lie outside `MIN ..= MAX`.
            #[must_use]
            #[inline]
            pub const fn is_out_of_range(self) -> bool {
                self.0 > Self::MAX.0 || self.0 < Self::MIN.0
            }

            /// Clamps a wide bit pattern into `MIN ..= MAX`.
            #[inline]
            const fn saturate(wide: $wide) -> Self {
                if wide > Self::MAX.0 as $wide {
                    Self::MAX
                } else if wide < Self::MIN.0 as $wide {
                    Self::MIN
                } else {
                    Self(wide as $repr)
                }
            }

            /// Checks that a wide bit pattern is in `MIN ..= MAX`.
            #[inline]
            const fn check(wide: $wide) -> Option<Self> {
                if wide > Self::MAX.0 as $wide || wide < Self::MIN.0 as $wide {
                    None
                } else {
                    Some(Self(wide as $repr))
                }
            }

            /// Adds, returning `None` if the result leaves `-pi/2 ..= pi/2`.
            #[must_use]
            #[inline]
            pub const fn checked_add(self, rhs: Self) -> Option<Self> {
                Self::check(self.cmp_key() as $wide + rhs.cmp_key() as $wide)
            }

            /// Adds, clamping at [`MIN`](Self::MIN) or [`MAX`](Self::MAX).
            ///
            /// The reason this family exists: tilting further than straight up
            /// leaves you looking straight up.
            #[must_use]
            #[inline]
            pub const fn saturating_add(self, rhs: Self) -> Self {
                Self::saturate(self.cmp_key() as $wide + rhs.cmp_key() as $wide)
            }

            /// Subtracts, returning `None` if the result leaves `-pi/2 ..= pi/2`.
            #[must_use]
            #[inline]
            pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
                Self::check(self.cmp_key() as $wide - rhs.cmp_key() as $wide)
            }

            /// Subtracts, clamping at [`MIN`](Self::MIN) or [`MAX`](Self::MAX).
            #[must_use]
            #[inline]
            pub const fn saturating_sub(self, rhs: Self) -> Self {
                Self::saturate(self.cmp_key() as $wide - rhs.cmp_key() as $wide)
            }

            /// Negates. Exact and total, since the range is symmetric.
            #[must_use]
            #[inline]
            pub const fn neg(self) -> Self {
                Self(-self.cmp_key())
            }

            /// The absolute value. Exact and total.
            #[must_use]
            #[inline]
            pub const fn abs(self) -> Self {
                let bits = self.cmp_key();
                Self(if bits < 0 { -bits } else { bits })
            }

            /// Returns `true` if this is below the horizon.
            #[must_use]
            #[inline]
            pub const fn is_negative(self) -> bool {
                self.cmp_key() < 0
            }

            /// Returns `true` if this is above the horizon.
            #[must_use]
            #[inline]
            pub const fn is_positive(self) -> bool {
                self.cmp_key() > 0
            }

            #[doc = concat!("Reinterprets this pitch as an [`", stringify!($angle), "`].")]
            ///
            /// Free: the two types share a scale, so for every value in range
            /// this is the identity on the bit pattern. A bit pattern from
            /// *outside* the range is clamped first, exactly as
            /// [`canonicalize`](Self::canonicalize) would -- this reads the
            /// pitch's value, so it cannot hand on a phase the type does not
            /// mean. Round-tripping raw bytes is [`to_bits`](Self::to_bits)'s
            /// job, not this one.
            #[must_use]
            #[inline]
            pub const fn to_angle(self) -> $angle {
                $angle::from_bits(self.cmp_key() as _)
            }

            #[doc = concat!("Clamps an [`", stringify!($angle), "`] into this range.")]
            ///
            /// The angle is read as a signed offset from zero, so a heading of
            /// 350 degrees becomes a pitch of -10 rather than saturating upward.
            #[must_use]
            #[inline]
            pub const fn from_angle(angle: $angle) -> Self {
                Self(angle.to_signed_bits()).canonicalize()
            }

            /// The angle as a phase across the full `u32` range.
            #[inline]
            const fn phase(self) -> u32 {
                // The stored bits are signed, so this sign-extends and then
                // truncates, which is the wrapping the phase space wants:
                // -MAX becomes three quarters of a turn.
                ((self.cmp_key() as i32 as u32) << $phase_shift)
            }
        }

        impl_shared!($name, $repr, " turn");
        impl_binop!($name, Add::add, AddAssign::add_assign, saturating_add);
        impl_binop!($name, Sub::sub, SubAssign::sub_assign, saturating_sub);
        impl_neg!($name, neg);
        impl_num_traits_shared!($name);

        #[cfg(feature = "num-traits")]
        impl ::num_traits::CheckedAdd for $name {
            #[inline]
            fn checked_add(&self, rhs: &Self) -> Option<Self> {
                (*self).checked_add(*rhs)
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::CheckedSub for $name {
            #[inline]
            fn checked_sub(&self, rhs: &Self) -> Option<Self> {
                (*self).checked_sub(*rhs)
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::Saturating for $name {
            #[inline]
            fn saturating_add(self, rhs: Self) -> Self {
                Self::saturating_add(self, rhs)
            }

            #[inline]
            fn saturating_sub(self, rhs: Self) -> Self {
                Self::saturating_sub(self, rhs)
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::SaturatingAdd for $name {
            #[inline]
            fn saturating_add(&self, rhs: &Self) -> Self {
                (*self).saturating_add(*rhs)
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::SaturatingSub for $name {
            #[inline]
            fn saturating_sub(&self, rhs: &Self) -> Self {
                (*self).saturating_sub(*rhs)
            }
        }
    };
}
pub(super) use define_pitch;
