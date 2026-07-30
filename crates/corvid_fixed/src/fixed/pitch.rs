//! Clamping angles covering a quarter turn either side of zero.
//!
//! A pitch is an [angle](super::angle) that stops instead of wrapping:
//! [`MAX`](Pitch16::MAX) is exactly `+pi/2` and [`MIN`](Pitch16::MIN) is exactly
//! `-pi/2`, both inclusive, and arithmetic saturates at them. That is what a
//! camera's vertical look wants — pitch past straight up should stay at straight
//! up, not flip the world over — and equally what a latitude, an elevation, or a
//! slope wants.
//!
//! # Units shared with yaw
//!
//! A pitch uses the *same* scale as the [angle](super::angle) of matching width:
//! one turn is `2^BITS`, so [`Pitch16`] and [`Angle16`](crate::Angle16) resolve
//! to the same 1/65536 of a turn and convert between each other by
//! [`to_angle`](Pitch16::to_angle) with no arithmetic at all. A camera's two
//! angles are then directly comparable, and the trigonometry below is literally
//! the same code the wrapping angles use.
//!
//! The cost is one bit of the storage type: `Pitch16` only ever holds
//! `-16384 ..= 16384` of `i16`'s range. Bit patterns outside that are accepted —
//! keeping [`from_bits`](Pitch16::from_bits) the exact inverse of
//! [`to_bits`](Pitch16::to_bits), so `bytemuck` and `serde` stay faithful — and
//! read as `±pi/2`, exactly as though they had been clamped. Arithmetic
//! canonicalizes, so no operation ever produces one.

use super::angle::{Angle8, Angle16, Angle32};
use super::factor::{Factor8, Factor16, Factor32};
use super::macros::{define_newtype, impl_binop, impl_neg, impl_num_traits_shared, impl_shared};
use super::point::I24F8;
use super::signed::{Signed8, Signed16, Signed32};
use crate::trig;

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

            /// Converts from turns, or returns `None` if outside
            /// `-0.25 ..= 0.25`.
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
            /// Free: the two types share a scale, so this is the identity on the
            /// bit pattern.
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
                // Sign extension then truncation is the wrapping the phase space
                // wants: -MAX becomes three quarters of a turn.
                ((self.cmp_key() as u32) << $phase_shift)
            }

            #[doc = concat!("The sine, as a [`", stringify!($signed), "`].")]
            ///
            /// Correctly rounded, and exactly `±1` at `±pi/2`. Spans the whole
            /// output range, since pitch covers a full quarter turn either way.
            #[must_use]
            #[inline]
            pub const fn sin(self) -> $signed {
                let scale = $signed::MAX.to_bits() as i64;
                $signed::from_bits(
                    trig::q_to_snorm(trig::sin_q(self.phase()), scale) as $signed_repr
                )
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
                    trig::q_to_snorm(trig::cos_q(self.phase()), scale) as $signed_repr
                )
            }

            /// The sine and cosine together.
            #[must_use]
            #[inline]
            pub const fn sin_cos(self) -> ($signed, $signed) {
                (self.sin(), self.cos())
            }

            /// The sine, approximately. Worst-case error is `1.1e-3`.
            #[must_use]
            #[inline]
            pub const fn sin_fast(self) -> $signed {
                let scale = $signed::MAX.to_bits() as i64;
                $signed::from_bits(
                    trig::q_to_snorm(trig::sin_fast_q(self.phase()), scale) as $signed_repr
                )
            }

            /// The cosine, approximately. See [`sin_fast`](Self::sin_fast).
            #[must_use]
            #[inline]
            pub const fn cos_fast(self) -> $signed {
                let scale = $signed::MAX.to_bits() as i64;
                let phase = self.phase().wrapping_add(1 << 30);
                $signed::from_bits(trig::q_to_snorm(trig::sin_fast_q(phase), scale) as $signed_repr)
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
            /// Exact at `±1`, which map to `±pi/2`.
            #[must_use]
            #[inline]
            pub const fn asin(value: $signed) -> Self {
                const SCALE: i64 = $signed::MAX.to_bits() as i64;
                const RECIPROCAL: i128 = trig::snorm_reciprocal(SCALE);
                let bits = trig::asin_bits(
                    value.canonicalize().to_bits() as i64,
                    SCALE,
                    RECIPROCAL,
                    <$repr>::BITS,
                );
                Self(bits as $repr)
            }

            /// The arctangent of `y / x`, clamped to `-pi/2 ..= pi/2`.
            ///
            /// Scale invariant, like [`atan2`](crate::Angle16::atan2), but folded
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
                Self(trig::atan2_bits(y, mirrored, <$repr>::BITS) as $repr).canonicalize()
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

define_pitch! {
    /// An 8-bit clamping pitch, covering `-pi/2 ..= pi/2`.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i8` |
    /// | Range | `-64 ..= 64`, denoting `-90 ..= 90` degrees |
    /// | Resolution | `1/256` turn, or 1.40625 degrees |
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::{Pitch8, Signed8};
    ///
    /// assert_eq!(Pitch8::from_degrees(90.0), Pitch8::MAX);
    /// assert_eq!(Pitch8::from_degrees(200.0), Pitch8::MAX);
    /// assert_eq!(Pitch8::MAX.sin(), Signed8::MAX);
    /// ```
    Pitch8(i8) {
        wide: i32,
        phase_shift: 24,
        angle: Angle8,
        signed: Signed8(i8),
        factor: Factor8,
    }
}

define_pitch! {
    /// A 16-bit clamping pitch, covering `-pi/2 ..= pi/2`.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i16` |
    /// | Range | `-16384 ..= 16384`, denoting `-90 ..= 90` degrees |
    /// | Resolution | `1/65536` turn, or about 0.0055 degrees |
    ///
    /// The camera pitch to reach for, paired with an
    /// [`Angle16`](crate::Angle16) yaw.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::{Angle16, Pitch16, Signed16};
    ///
    /// // Looking up past vertical stays at vertical.
    /// let mut pitch = Pitch16::from_degrees(85.0);
    /// pitch += Pitch16::from_degrees(20.0);
    /// assert_eq!(pitch, Pitch16::MAX);
    /// assert_eq!(pitch.to_degrees(), 90.0);
    ///
    /// // The endpoints are exact, in both directions.
    /// assert_eq!(Pitch16::MAX.sin(), Signed16::MAX);
    /// assert_eq!(Pitch16::MAX.cos(), Signed16::ZERO);
    /// assert_eq!(Pitch16::asin(Signed16::MAX), Pitch16::MAX);
    /// assert_eq!(Pitch16::asin(Signed16::ZERO), Pitch16::ZERO);
    ///
    /// // The cosine of a pitch is never negative, which is what makes it safe
    /// // to build a direction vector from.
    /// assert!(!Pitch16::MIN.cos().is_negative());
    ///
    /// // Yaw and pitch share a scale, so converting is free.
    /// assert_eq!(Pitch16::from_degrees(45.0).to_angle(), Angle16::from_degrees(45.0));
    /// assert_eq!(Pitch16::from_angle(Angle16::from_degrees(350.0)).to_degrees().round(), -10.0);
    /// ```
    Pitch16(i16) {
        wide: i32,
        phase_shift: 16,
        angle: Angle16,
        signed: Signed16(i16),
        factor: Factor16,
    }
}

define_pitch! {
    /// A 32-bit clamping pitch, covering `-pi/2 ..= pi/2`.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i32` |
    /// | Range | `-2^30 ..= 2^30`, denoting `-90 ..= 90` degrees |
    /// | Resolution | `1/2^32` turn, or about `8.4e-8` degrees |
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::{Pitch32, Signed32};
    ///
    /// let pitch = Pitch32::from_degrees(30.0);
    /// assert!((pitch.sin().to_f64() - 0.5).abs() < 1e-9);
    /// assert_eq!(Pitch32::asin(pitch.sin()).to_degrees().round(), 30.0);
    /// ```
    Pitch32(i32) {
        wide: i64,
        phase_shift: 0,
        angle: Angle32,
        signed: Signed32(i32),
        factor: Factor32,
    }
}
