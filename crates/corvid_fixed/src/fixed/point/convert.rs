//! Conversions between scalars of different scales.
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

use super::{I2F30, I16F16, I24F8, I48F16};

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
pub(super) const fn divide(numerator: i64, denominator: i64) -> i64 {
    // `unsigned_abs` rather than `abs`, which overflows on `i64::MIN` -- a
    // value no call site reaches, and a panic the workspace forbids being one
    // branch away from is not worth the shorter spelling.
    let half = (denominator.unsigned_abs() / 2) as i64;
    let bump = if numerator < 0 { -half } else { half };
    (numerator + bump) / denominator
}

impl I16F16 {
    /// The same value at thirty fractional bits, saturating.
    ///
    /// Fourteen more fractional bits, and fourteen fewer of range: [`I16F16`]
    /// reaches +/-32.7 km at 15 microns and [`I2F30`] reaches +/-2 at
    /// 9.3e-10. Almost every [`I16F16`] therefore has no [`I2F30`] to become,
    /// and clamping is the answer for the reason the rest of this module gives.
    ///
    /// The conversion a colour channel makes. A linear channel is nominally in
    /// `[0, 1]` and held in a type with room for 32 768 of them; the moment it
    /// is about to go through an operation that needs resolution rather than
    /// range -- a cube root, for Oklab -- it wants the narrow, fine type
    /// instead, and a channel past 2 was already outside any gamut.
    ///
    /// ```
    /// use corvid_fixed::{I2F30, I16F16};
    ///
    /// assert_eq!(I16F16::from_f64(0.5).to_i2f30(), I2F30::from_f64(0.5));
    ///
    /// // Past what an `I2F30` holds, it clamps.
    /// assert_eq!(I16F16::MAX.to_i2f30(), I2F30::MAX);
    /// assert_eq!(I16F16::MIN.to_i2f30(), I2F30::MIN);
    /// ```
    #[must_use]
    #[inline]
    pub const fn to_i2f30(self) -> I2F30 {
        I2F30::saturate((self.to_bits() as i64) << 14)
    }
}

impl I2F30 {
    /// The same value at sixteen fractional bits, rounded to nearest.
    ///
    /// The way back from [`I16F16::to_i2f30`], and the one direction of the
    /// pair that cannot clamp: every [`I2F30`] is inside `+/-2` and so inside
    /// [`I16F16`]'s range. Fourteen fractional bits go, which is a rounding
    /// rather than a truncation -- halves away from zero, as everywhere else
    /// in this crate -- so a round trip through the two is off by at most the
    /// destination's own last bit rather than always downward.
    ///
    /// ```
    /// use corvid_fixed::{I2F30, I16F16};
    ///
    /// assert_eq!(I2F30::from_f64(0.5).to_i16f16(), I16F16::from_f64(0.5));
    ///
    /// // Rounded, not truncated: half a step of the destination goes up.
    /// let half_step = I2F30::from_bits(1 << 13);
    /// assert_eq!(half_step.to_i16f16(), I16F16::from_bits(1));
    /// ```
    #[must_use]
    #[inline]
    pub const fn to_i16f16(self) -> I16F16 {
        let bits = self.to_bits() as i64;
        let half = 1 << 13;
        let rounded = if bits >= 0 {
            (bits + half) >> 14
        } else {
            -((-bits + half) >> 14)
        };
        I16F16::saturate(rounded)
    }
}

impl I16F16 {
    /// The fraction an 8-bit code denotes, `code / 255`.
    ///
    /// The [`UNORM`] convention: 0 is none of it and 255 is all of it, with the
    /// step between codes 1/255 rather than 1/256, so that both ends are exact.
    /// A texture, a palette and a colour channel all arrive this way, and the
    /// conversion is here rather than in each of them because getting it wrong
    /// by one step is invisible until two of them disagree.
    ///
    /// [`to_unorm8`](Self::to_unorm8) is its inverse, exactly, for all 256
    /// codes.
    ///
    /// [`UNORM`]: https://registry.khronos.org/vulkan/specs/latest/html/vkspec.html#fundamentals-fixedconv
    ///
    /// ```
    /// use corvid_fixed::I16F16;
    ///
    /// assert_eq!(I16F16::from_unorm8(0), I16F16::ZERO);
    /// assert_eq!(I16F16::from_unorm8(255), I16F16::ONE);
    ///
    /// // And every code survives the trip back.
    /// assert!((0..=255).all(|code| I16F16::from_unorm8(code).to_unorm8() == code));
    /// ```
    #[must_use]
    #[inline]
    pub const fn from_unorm8(code: u8) -> Self {
        let numerator = (code as i64) << Self::FRAC_BITS;
        Self::saturate(divide(numerator, 255))
    }

    /// The 8-bit code this fraction denotes, rounded and clamped.
    ///
    /// The inverse of [`from_unorm8`](Self::from_unorm8). Values outside
    /// `0.0 ..= 1.0` clamp to the ends rather than wrapping, because a colour
    /// channel that came back as its own complement is the worst answer
    /// available and an out-of-range one is ordinary -- an HDR value on its way
    /// to a display, or a blend that overshot.
    ///
    /// ```
    /// use corvid_fixed::I16F16;
    ///
    /// assert_eq!(I16F16::ONE.to_unorm8(), 255);
    /// assert_eq!(I16F16::from_f64(-1.0).to_unorm8(), 0);
    /// assert_eq!(I16F16::from_f64(2.0).to_unorm8(), 255);
    /// ```
    #[must_use]
    #[inline]
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the value is compared against both ends before the cast, which is what makes the narrowing exact"
    )]
    pub const fn to_unorm8(self) -> u8 {
        let scaled = divide((self.to_bits() as i64) * 255, 1 << Self::FRAC_BITS);
        if scaled <= 0 {
            0
        } else if scaled >= 255 {
            255
        } else {
            scaled as u8
        }
    }
}
