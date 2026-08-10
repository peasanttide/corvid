//! Conversions between the fixed-point scalars.
//!
//! Each pair here changes the binary scale, which is a widening of the
//! fraction and a narrowing of the range at the same time -- so every one of
//! them can clamp, and none is a `From`. A conversion that silently wrapped
//! would come back with the opposite sign, which is the one answer worse than
//! a clamped one.

use super::{I16F16, I24F8};

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
}
