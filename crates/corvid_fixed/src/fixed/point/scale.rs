//! Arithmetic with one normalized operand.
//!
//! Split from [`convert`](super::convert) because these change a value's
//! magnitude rather than its scale: the other operand is a share of one -- an
//! axis, a sine, a matrix coefficient -- and what comes back is the type it
//! started as. Every one of them widens, rounds once and saturates, which is
//! the part a caller composing a multiply and a divide of its own gets wrong in
//! the middle.

use super::{I2F30, I16F16, I24F8, divide, divide_wide, narrow_i64};
use crate::{Signed16, Signed32};

impl I24F8 {
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

impl I2F30 {
    /// `self` scaled by `factor`, which runs `-1.0 ..= 1.0`.
    ///
    /// The [`Signed32`] companion to
    /// [`I16F16::saturating_mul_signed16`](I16F16::saturating_mul_signed16),
    /// at the width a sine or a cosine comes back at. Taking a polar
    /// coordinate apart is the caller: a magnitude times the cosine of an angle
    /// is a Cartesian component, and both operands are already the types this
    /// crate hands out.
    ///
    /// Exact at the ends and at rest, and the product of two values inside
    /// `+/-2` and `+/-1` cannot leave the range, so the saturation is a
    /// formality rather than a case.
    ///
    /// ```
    /// use corvid_fixed::{I2F30, Signed32};
    ///
    /// let chroma = I2F30::from_f64(0.5);
    ///
    /// assert_eq!(chroma.saturating_mul_signed32(Signed32::MAX), chroma);
    /// assert_eq!(chroma.saturating_mul_signed32(Signed32::MIN), -chroma);
    /// assert_eq!(chroma.saturating_mul_signed32(Signed32::ZERO), I2F30::ZERO);
    /// ```
    #[must_use]
    #[inline]
    pub const fn saturating_mul_signed32(self, factor: Signed32) -> Self {
        // `canonicalize` first, so the `SNORM` denormal -- the second bit
        // pattern for `-1.0` -- cannot push the product one step outside the
        // range before the rounding sees it.
        let numerator = (self.to_bits() as i128) * (factor.canonicalize().to_bits() as i128);
        let denominator = Signed32::MAX.to_bits() as i128;
        Self::saturate(narrow_i64(divide_wide(numerator, denominator)))
    }

    /// One row of a Q30 matrix applied to a Q30 triple.
    ///
    /// The coefficients are raw bit patterns at this type's own scale rather
    /// than [`I2F30`]s, because a matrix of interest does not fit one: the
    /// Oklab transfer reaches 4.077, and this type stops at 2. They are the
    /// same scale all the same -- `1 << 30` is one -- so the caller writes its
    /// matrix down the way it writes any other constant here.
    ///
    /// The three products are accumulated at full width before the single
    /// rounding, which is the reason this is one operation rather than three
    /// multiplies and two adds: a Q30 coefficient against a Q30 value is a Q60
    /// product, and three of them summed reach past an `i64` -- so a caller
    /// composing the pieces itself would either overflow or round three times.
    ///
    /// ```
    /// use corvid_fixed::I2F30;
    ///
    /// // The identity row picks its own component out.
    /// let one = 1 << 30;
    /// let values = [I2F30::from_f64(0.25), I2F30::from_f64(0.5), I2F30::ZERO];
    /// assert_eq!(I2F30::dot_q30([one, 0, 0], values), values[0]);
    ///
    /// // And a row of halves is the average of the three.
    /// let half = one / 2;
    /// assert_eq!(
    ///     I2F30::dot_q30([half, half, half], values),
    ///     I2F30::from_f64(0.375)
    /// );
    /// ```
    #[must_use]
    #[inline]
    pub const fn dot_q30(coefficients: [i64; 3], values: [Self; 3]) -> Self {
        let sum = coefficients[0] as i128 * values[0].to_bits() as i128
            + coefficients[1] as i128 * values[1].to_bits() as i128
            + coefficients[2] as i128 * values[2].to_bits() as i128;
        // The quotient is inside `i64` whenever the row is, which the caller
        // owns; the narrowing and the `saturate` are what make that a clamp
        // rather than a wrap if it is not.
        Self::saturate(narrow_i64(divide_wide(sum, 1 << 30)))
    }
}
