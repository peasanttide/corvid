//! Conversions between the point widths.
//!
//! Three behaviours, distinguished on purpose:
//!
//! - **Exact.** The target holds at least as much range *and* resolution, so
//!   the conversion is a widening shift that loses nothing.
//! - **Total but lossy.** [`FinePoint::to_global`] cannot fail, because
//!   `I16F16`'s whole +/-32.7 km range fits inside `I24F8`'s +/-8388 km, but it
//!   drops 8 fractional bits -- 15.26 um becomes 3.9 mm. Rounded once, never
//!   truncated.
//! - **Range-checked.** Returns `None` when the value does not fit. Only
//!   *range* failure produces `None`; magnitude is never silently discarded.
//!
//! [`GlobalFinePoint::to_fine`] is the important one. Because `I48F16` and
//! `I16F16` both carry 16 fractional bits, it is a **pure range check on the
//! integer part -- `i64 as i32` after a bounds test, with no rounding at all**.
//! The near-field conversion a renderer runs thousands of times per frame is
//! exact by construction, not exact within a tolerance.

use corvid_fixed::{I16F16, I24F8, I48F16, Signed32};

use crate::{Direction, FinePoint, GlobalFinePoint, GlobalPoint};

/// Rounds `bits >> shift` half away from zero.
///
/// The rounding happens on the *unsigned* magnitude. Adding the half-step to a
/// signed `bits` overflows for the top 2^(shift-1) patterns of `i64` -- and
/// [`I48F16`] reaches them, because its own saturating arithmetic lands on
/// `i64::MAX` by design, which a narrowing conversion then has to be able to
/// look at.
#[inline]
const fn shift_round(bits: i64, shift: u32) -> i64 {
    let half = 1u64 << (shift - 1);
    // `|bits| <= 2^63` and `half < 2^63`, so the sum fits `u64`; the shift then
    // brings it back well inside `i64`.
    let rounded = ((bits.unsigned_abs() + half) >> shift) as i64;
    if bits >= 0 { rounded } else { -rounded }
}

/// Narrows an `i64` to an `i32`, or `None` if it does not fit.
#[inline]
const fn narrow(bits: i64) -> Option<i32> {
    if bits > i32::MAX as i64 || bits < i32::MIN as i64 {
        None
    } else {
        Some(bits as i32)
    }
}

impl GlobalPoint {
    /// Widens to the full-range, full-resolution type. Exact.
    ///
    /// `I24F8` and `I48F16` differ by 8 fractional bits, so this is a `<< 8`.
    #[must_use]
    #[inline]
    pub const fn to_global_fine(self) -> GlobalFinePoint {
        let [x, y, z] = self.to_array();
        GlobalFinePoint::new(
            I48F16::from_bits((x.to_bits() as i64) << 8),
            I48F16::from_bits((y.to_bits() as i64) << 8),
            I48F16::from_bits((z.to_bits() as i64) << 8),
        )
    }

    /// Narrows to the near-field type, or `None` if out of range.
    ///
    /// Exact in resolution -- `I24F8` to `I16F16` *widens* the fraction -- so
    /// range is the only thing that can fail.
    #[must_use]
    #[inline]
    pub const fn to_fine(self) -> Option<FinePoint> {
        let [x, y, z] = self.to_array();
        match (
            narrow((x.to_bits() as i64) << 8),
            narrow((y.to_bits() as i64) << 8),
            narrow((z.to_bits() as i64) << 8),
        ) {
            (Some(x), Some(y), Some(z)) => Some(FinePoint::new(
                I16F16::from_bits(x),
                I16F16::from_bits(y),
                I16F16::from_bits(z),
            )),
            _ => None,
        }
    }
}

impl FinePoint {
    /// Widens to the full-range type. Exact.
    ///
    /// Both types carry 16 fractional bits, so this is a widening of the bit
    /// pattern and nothing more.
    #[must_use]
    #[inline]
    pub const fn to_global_fine(self) -> GlobalFinePoint {
        let [x, y, z] = self.to_array();
        GlobalFinePoint::new(
            I48F16::from_bits(x.to_bits() as i64),
            I48F16::from_bits(y.to_bits() as i64),
            I48F16::from_bits(z.to_bits() as i64),
        )
    }

    /// Converts to the object-scale type. Total, and lossy.
    ///
    /// Cannot fail -- `I16F16`'s whole +/-32.7 km range fits inside `I24F8`'s
    /// +/-8388 km -- but drops 8 fractional bits, taking the resolution from
    /// 15.26 um to 3.9 mm. Rounded once, never truncated.
    #[must_use]
    #[inline]
    pub const fn to_global(self) -> GlobalPoint {
        let [x, y, z] = self.to_array();
        GlobalPoint::new(
            I24F8::from_bits(shift_round(x.to_bits() as i64, 8) as i32),
            I24F8::from_bits(shift_round(y.to_bits() as i64, 8) as i32),
            I24F8::from_bits(shift_round(z.to_bits() as i64, 8) as i32),
        )
    }
}

impl GlobalFinePoint {
    /// This point, unchanged.
    ///
    /// The identity widening. It exists so that code generic over a position
    /// type -- `corvid_transform`'s macro, which widens both tiers to `I48F16`
    /// before it subtracts -- can call one method name on all of them.
    #[must_use]
    #[inline]
    pub const fn to_global_fine(self) -> Self {
        self
    }

    /// Narrows to the near-field type, or `None` if out of range.
    ///
    /// **Bit-exact.** Both types carry 16 fractional bits, so this is a bounds
    /// test and an `i64 as i32` -- there is no rounding step for anything to be
    /// lost in. This is what makes the world->eye conversion in
    /// `corvid_transform` exact by construction.
    #[must_use]
    #[inline]
    pub const fn to_fine(self) -> Option<FinePoint> {
        let [x, y, z] = self.to_array();
        match (
            narrow(x.to_bits()),
            narrow(y.to_bits()),
            narrow(z.to_bits()),
        ) {
            (Some(x), Some(y), Some(z)) => Some(FinePoint::new(
                I16F16::from_bits(x),
                I16F16::from_bits(y),
                I16F16::from_bits(z),
            )),
            _ => None,
        }
    }

    /// Narrows to the object-scale type, or `None` if out of range.
    ///
    /// Drops 8 fractional bits, rounded once.
    #[must_use]
    #[inline]
    pub const fn to_global(self) -> Option<GlobalPoint> {
        let [x, y, z] = self.to_array();
        match (
            narrow(shift_round(x.to_bits(), 8)),
            narrow(shift_round(y.to_bits(), 8)),
            narrow(shift_round(z.to_bits(), 8)),
        ) {
            (Some(x), Some(y), Some(z)) => Some(GlobalPoint::new(
                I24F8::from_bits(x),
                I24F8::from_bits(y),
                I24F8::from_bits(z),
            )),
            _ => None,
        }
    }
}

/// Rescales a [`Signed32`] component -- a value over `2^31 - 1` -- onto a
/// power-of-two fractional scale, rounded once.
///
/// Takes the [`Signed32`] rather than its bits so it can canonicalize: the
/// `SNORM` convention spends `i32::MIN` and `-(2^31 - 1)` on the same `-1.0`,
/// and two components that compare equal must convert alike.
#[inline]
const fn direction_component(value: Signed32, frac: u32) -> i64 {
    let bits = value.canonicalize().to_bits();
    let scaled = (bits as i64) << frac;
    let denominator = Signed32::MAX.to_bits() as i64;
    if scaled >= 0 {
        (2 * scaled + denominator) / (2 * denominator)
    } else {
        -((-2 * scaled + denominator) / (2 * denominator))
    }
}

impl Direction {
    /// The unit direction as a near-field offset of length one. Total.
    #[must_use]
    #[inline]
    pub const fn to_fine(self) -> FinePoint {
        let [x, y, z] = self.to_array();
        FinePoint::new(
            I16F16::from_bits(direction_component(x, 16) as i32),
            I16F16::from_bits(direction_component(y, 16) as i32),
            I16F16::from_bits(direction_component(z, 16) as i32),
        )
    }

    /// The unit direction as an object-scale offset of length one. Total.
    #[must_use]
    #[inline]
    pub const fn to_global(self) -> GlobalPoint {
        let [x, y, z] = self.to_array();
        GlobalPoint::new(
            I24F8::from_bits(direction_component(x, 8) as i32),
            I24F8::from_bits(direction_component(y, 8) as i32),
            I24F8::from_bits(direction_component(z, 8) as i32),
        )
    }

    /// The unit direction as a world-scale offset of length one. Total.
    #[must_use]
    #[inline]
    pub const fn to_global_fine(self) -> GlobalFinePoint {
        let [x, y, z] = self.to_array();
        GlobalFinePoint::new(
            I48F16::from_bits(direction_component(x, 16)),
            I48F16::from_bits(direction_component(y, 16)),
            I48F16::from_bits(direction_component(z, 16)),
        )
    }
}

impl From<GlobalPoint> for GlobalFinePoint {
    #[inline]
    fn from(point: GlobalPoint) -> Self {
        point.to_global_fine()
    }
}

impl From<FinePoint> for GlobalFinePoint {
    #[inline]
    fn from(point: FinePoint) -> Self {
        point.to_global_fine()
    }
}

impl From<FinePoint> for GlobalPoint {
    #[inline]
    fn from(point: FinePoint) -> Self {
        point.to_global()
    }
}

/// The error from a range-checked narrowing.
///
/// Carries no detail on purpose: the only way a narrowing fails is that the
/// value lies outside the target's range, and the value itself is still in the
/// caller's hand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OutOfRange;

impl core::fmt::Display for OutOfRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("value is outside the target type's range")
    }
}

impl TryFrom<GlobalFinePoint> for FinePoint {
    type Error = OutOfRange;

    #[inline]
    fn try_from(point: GlobalFinePoint) -> Result<Self, Self::Error> {
        point.to_fine().ok_or(OutOfRange)
    }
}

impl TryFrom<GlobalFinePoint> for GlobalPoint {
    type Error = OutOfRange;

    #[inline]
    fn try_from(point: GlobalFinePoint) -> Result<Self, Self::Error> {
        point.to_global().ok_or(OutOfRange)
    }
}

impl TryFrom<GlobalPoint> for FinePoint {
    type Error = OutOfRange;

    #[inline]
    fn try_from(point: GlobalPoint) -> Result<Self, Self::Error> {
        point.to_fine().ok_or(OutOfRange)
    }
}
