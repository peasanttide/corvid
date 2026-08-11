//! Operations between scalars of different scales.
//!
//! Every one of these changes the binary scale, which is where a fixed-point
//! type is at its most fragile: a widening of the fraction is a narrowing of
//! the range at the same time, so all of them can clamp and none is a `From`.
//! A conversion that silently wrapped would come back with the opposite sign,
//! which is the one answer worse than a clamped one.
//!
//! [`I24F8::squared`] and [`I48F16::root`] are the pair that makes a distance
//! comparison cheap. Squaring doubles the scale exactly -- Q8 times Q8 is Q16 --
//! so the wide type is where a squared length belongs, and an integer square
//! root of a Q16 *is* the Q8 answer with no rescaling step to lose anything in.
//! That is what lets a shape crate compare and intersect distances with no
//! square root in the common case and no floating point in any case.

use super::{I16F16, I24F8, I48F16};
use crate::{Signed16, Signed32};

impl I24F8 {
    /// The same value at sixteen fractional bits, saturating.
    ///
    /// The two types share nothing but a sign: [`I24F8`] reaches +/-8388 km at
    /// 3.9 mm and [`I16F16`] reaches +/-32.7 km at 15 microns, so a widening
    /// of the fraction is a narrowing of the range and the far half of a
    /// [`I24F8`] has no [`I16F16`] to become. It clamps rather than wrapping,
    /// for the reason every conversion here does: a value that wrapped would
    /// come back with the opposite sign.
    ///
    /// This is the conversion a tangent needs. [`tan`](crate::Pitch32::tan)
    /// answers a Q8 because a tangent is unbounded and Q8 has the range for
    /// it; a caller working in Q16 wants the same number at its own scale.
    ///
    /// ```
    /// use corvid_fixed::{I16F16, I24F8};
    ///
    /// assert_eq!(I24F8::from_f64(1.5).to_i16f16(), I16F16::from_f64(1.5));
    ///
    /// // Past what a `I16F16` holds, it clamps.
    /// assert_eq!(I24F8::MAX.to_i16f16(), I16F16::MAX);
    /// assert_eq!(I24F8::MIN.to_i16f16(), I16F16::MIN);
    /// ```
    #[must_use]
    #[inline]
    pub const fn to_i16f16(self) -> I16F16 {
        // Eight more fractional bits, in a width that holds the shift. The
        // saturating narrow is `I16F16`'s own, so the clamp is the one every
        // other operation on that type uses.
        I16F16::saturate((self.to_bits() as i64) << 8)
    }

    /// `self` scaled by `factor`, which runs `-1.0 ..= 1.0`.
    ///
    /// The operation an axis is one of: a control reports how far along its
    /// range it is, and a game says what a full deflection is worth. The two
    /// scales do not line up -- a [`Signed16`] is `bits / 32767` and this type
    /// is a power of two -- so crossing between them is a multiply and a
    /// rounded divide rather than a shift.
    ///
    /// Exact at the ends and at rest: [`Signed16::MAX`] gives `self`,
    /// [`Signed16::MIN`] gives `-self` and [`Signed16::ZERO`] gives zero.
    /// Everything between is the product rounded once, and the rounding is
    /// symmetric, so a push one way is the same size as the same push back --
    /// which matters because what a game builds out of an axis is hashed by
    /// every peer.
    ///
    /// Saturating in the one case a two's-complement range cannot answer: the
    /// negation of [`MIN`](Self::MIN) is not a value, so `MIN` scaled by
    /// [`Signed16::MIN`] gives [`MAX`](Self::MAX) rather than wrapping.
    ///
    /// ```
    /// use corvid_fixed::{I24F8, Signed16};
    ///
    /// let full = I24F8::from_f64(100.0);
    /// assert_eq!(full.saturating_mul_signed16(Signed16::MAX), full);
    /// assert_eq!(full.saturating_mul_signed16(Signed16::MIN), -full);
    /// assert_eq!(full.saturating_mul_signed16(Signed16::ZERO), I24F8::ZERO);
    ///
    /// // Whatever it rounds to, it rounds there in both directions.
    /// let half = Signed16::from_bits(16_384);
    /// assert_eq!(
    ///     full.saturating_mul_signed16(-half),
    ///     -full.saturating_mul_signed16(half),
    /// );
    ///
    /// // The one asymmetry is the range's own.
    /// assert_eq!(I24F8::MIN.saturating_mul_signed16(Signed16::MIN), I24F8::MAX);
    /// ```
    #[must_use]
    #[inline]
    pub const fn saturating_mul_signed16(self, factor: Signed16) -> Self {
        // `canonicalize` first, so the `SNORM` denormal -- the second bit
        // pattern for `-1.0` -- cannot push the product one step outside the
        // range before the rounding sees it.
        let numerator = (self.to_bits() as i64) * (factor.canonicalize().to_bits() as i64);
        Self::saturate(divide(numerator, Signed16::MAX.to_bits() as i64))
    }

    /// The square, at the doubled scale. **Exact**, always.
    ///
    /// A Q8 times a Q8 is a Q16, and the widest square an [`I24F8`] has is
    /// `2^62` -- inside an `i64` with a bit to spare -- so this is the one
    /// conversion here that cannot clamp. [`I48F16::root`] takes it back.
    ///
    /// ```
    /// use corvid_fixed::{I24F8, I48F16};
    ///
    /// assert_eq!(I24F8::from(3).squared(), I48F16::from(9));
    /// assert_eq!(I24F8::from(-3).squared(), I48F16::from(9));
    ///
    /// // The far corner of the range, where a same-width square would wrap.
    /// assert_eq!(I24F8::MAX.squared().root(), I24F8::MAX);
    /// ```
    #[must_use]
    #[inline]
    pub const fn squared(self) -> I48F16 {
        let bits = self.to_bits() as i64;
        I48F16::from_bits(bits * bits)
    }

    /// `self / divisor`, or [`None`] when the divisor is zero.
    ///
    /// The operation a cast is one of: a length over how much two directions
    /// agree, which is a distance. The divisor is a [`Signed32`], so dividing
    /// by it *lengthens* -- a plane seen at a glancing angle is further along
    /// the ray than it is away -- and the answer saturates rather than
    /// wrapping when the angle is glancing enough to push it past the range.
    ///
    /// ```
    /// use corvid_fixed::{I24F8, Signed32};
    ///
    /// let half = Signed32::from_f64(0.5);
    /// assert_eq!(I24F8::from(3).checked_div_signed32(half), Some(I24F8::from(6)));
    /// assert_eq!(I24F8::from(3).checked_div_signed32(Signed32::ZERO), None);
    ///
    /// // A ray parallel to nothing: dividing by one is the identity, exactly.
    /// assert_eq!(I24F8::MAX.checked_div_signed32(Signed32::MAX), Some(I24F8::MAX));
    /// ```
    #[must_use]
    #[inline]
    pub const fn checked_div_signed32(self, divisor: Signed32) -> Option<Self> {
        let denominator = divisor.canonicalize().to_bits() as i64;
        if denominator == 0 {
            return None;
        }
        // Q8 over Q31 is a Q-23, so the numerator carries the unit up front to
        // land back on a Q8. `2^31 x 2^31` is the widest it reaches, which is
        // an `i64` with a bit spare -- and the unit rather than a shift of 31,
        // because `Signed32`'s one is `2^31 - 1` and shifting would floor the
        // shortfall into a whole step in the last place.
        let numerator = (self.to_bits() as i64) * (Signed32::MAX.to_bits() as i64);
        Some(Self::saturate(divide(numerator, denominator)))
    }

    /// `self / divisor`, saturating at both ends including a zero divisor.
    ///
    /// A zero divisor answers [`MAX`](Self::MAX) or [`MIN`](Self::MIN) by the
    /// sign of the numerator, and zero over zero is zero -- the same reading
    /// [`recip`](Self::recip) gives, and the one a slab test wants when it has
    /// already decided that a ray parallel to a pair of faces is constrained by
    /// neither.
    #[must_use]
    #[inline]
    pub const fn saturating_div_signed32(self, divisor: Signed32) -> Self {
        match self.checked_div_signed32(divisor) {
            Some(quotient) => quotient,
            None if self.is_negative() => Self::MIN,
            None if self.is_positive() => Self::MAX,
            None => Self::ZERO,
        }
    }
}

impl I16F16 {
    /// The same value at eight fractional bits, rounded to nearest.
    ///
    /// The way back from [`I24F8::to_i16f16`], and the direction that cannot
    /// fail: [`I16F16`] reaches +/-32.7 km and [`I24F8`] reaches +/-8388 km, so
    /// widening the range while narrowing the fraction always fits. Eight
    /// fractional bits go, rounded rather than truncated.
    ///
    /// The conversion a mesh makes. A vertex holds a share of a scale, the
    /// scale is in [`I16F16`] metres because that is the resolution a size is
    /// written at, and what comes out is a position in the world's own
    /// [`I24F8`].
    ///
    /// ```
    /// use corvid_fixed::{I16F16, I24F8};
    ///
    /// assert_eq!(I16F16::from_f64(1.5).to_i24f8(), I24F8::from_f64(1.5));
    ///
    /// // Rounded: half a step of the destination goes away from zero.
    /// let half_step = I16F16::from_bits(1 << 7);
    /// assert_eq!(half_step.to_i24f8(), I24F8::from_bits(1));
    /// assert_eq!((-half_step).to_i24f8(), I24F8::from_bits(-1));
    /// ```
    #[must_use]
    #[inline]
    pub const fn to_i24f8(self) -> I24F8 {
        let bits = self.to_bits() as i64;
        let half = 1 << 7;
        let rounded = if bits >= 0 {
            (bits + half) >> 8
        } else {
            -((-bits + half) >> 8)
        };
        I24F8::saturate(rounded)
    }

    /// `self` scaled by `factor`, which runs `-1.0 ..= 1.0`.
    ///
    /// The operation an axis is one of: a control reports how far along its
    /// range it is, and a game says what a full deflection is worth. The two
    /// scales do not line up -- a [`Signed16`] is `bits / 32767` and this type
    /// is a power of two -- so crossing between them is a multiply and a
    /// rounded divide rather than a shift.
    ///
    /// Exact at the ends and at rest: [`Signed16::MAX`] gives `self`,
    /// [`Signed16::MIN`] gives `-self` and [`Signed16::ZERO`] gives zero.
    /// Everything between is the product rounded once, and the rounding is
    /// symmetric, so a push one way is the same size as the same push back --
    /// which matters because what a game builds out of an axis is hashed by
    /// every peer.
    ///
    /// Saturating in the one case a two's-complement range cannot answer: the
    /// negation of [`MIN`](Self::MIN) is not a value, so `MIN` scaled by
    /// [`Signed16::MIN`] gives [`MAX`](Self::MAX) rather than wrapping.
    ///
    /// ```
    /// use corvid_fixed::{I16F16, Signed16};
    ///
    /// let full = I16F16::from_f64(2.5);
    /// assert_eq!(full.saturating_mul_signed16(Signed16::MAX), full);
    /// assert_eq!(full.saturating_mul_signed16(Signed16::MIN), -full);
    /// assert_eq!(full.saturating_mul_signed16(Signed16::ZERO), I16F16::ZERO);
    ///
    /// // Whatever it rounds to, it rounds there in both directions.
    /// let half = Signed16::from_bits(16_384);
    /// assert_eq!(
    ///     full.saturating_mul_signed16(-half),
    ///     -full.saturating_mul_signed16(half),
    /// );
    ///
    /// // The one asymmetry is the range's own.
    /// assert_eq!(I16F16::MIN.saturating_mul_signed16(Signed16::MIN), I16F16::MAX);
    /// ```
    #[must_use]
    #[inline]
    pub const fn saturating_mul_signed16(self, factor: Signed16) -> Self {
        // `canonicalize` first, so the `SNORM` denormal -- the second bit
        // pattern for `-1.0` -- cannot push the product one step outside the
        // range before the rounding sees it.
        let numerator = (self.to_bits() as i64) * (factor.canonicalize().to_bits() as i64);
        Self::saturate(divide(numerator, Signed16::MAX.to_bits() as i64))
    }
}

impl I48F16 {
    /// The square root, at the halved scale: the inverse of
    /// [`I24F8::squared`].
    ///
    /// An integer square root of a Q16 *is* the Q8 answer -- halving the scale
    /// is what taking a square root does to it -- so unlike
    /// [`sqrt`](Self::sqrt) there is no rescaling here and nothing to lose in
    /// one. A negative answers [`ZERO`](I24F8::ZERO), matching `sqrt`.
    ///
    /// ```
    /// use corvid_fixed::{I24F8, I48F16};
    ///
    /// assert_eq!(I48F16::from(9).root(), I24F8::from(3));
    /// assert_eq!(I48F16::from(-1).root(), I24F8::ZERO);
    ///
    /// // Half a step up rounds to the nearer of the two.
    /// assert_eq!(I48F16::from_f64(2.0).root(), I24F8::from_f64(1.414_062_5));
    /// ```
    #[must_use]
    #[inline]
    pub const fn root(self) -> I24F8 {
        if self.to_bits() <= 0 {
            return I24F8::ZERO;
        }
        let squared = self.to_bits().unsigned_abs();
        let root = squared.isqrt();
        // Round up when the true root is past the halfway point, which happens
        // exactly when the remainder exceeds the root.
        let rounded = if squared - root * root > root {
            root + 1
        } else {
            root
        };
        I24F8::saturate(rounded as i64)
    }
}

/// A quotient, rounded to nearest with halves away from zero.
///
/// Rust's integer division truncates toward zero, which turns every sub-unit
/// shortfall into a whole step in the last place -- systematic, in the same
/// direction every time, and enough to put a ray's hit under the surface it was
/// cast at. The caller has already rejected a zero denominator.
#[inline]
const fn divide(numerator: i64, denominator: i64) -> i64 {
    // `unsigned_abs` rather than `abs`, which overflows on `i64::MIN` -- a
    // value no call site reaches, and a panic the workspace forbids being one
    // branch away from is not worth the shorter spelling.
    let half = (denominator.unsigned_abs() / 2) as i64;
    let bump = if numerator < 0 { -half } else { half };
    (numerator + bump) / denominator
}
