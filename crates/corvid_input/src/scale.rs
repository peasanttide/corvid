//! Turning an axis into a quantity.
//!
//! An [`Analog`](crate::Analog) axis is a [`Signed16`]: `SNORM`, so the value
//! is `bits / 32767` and the ends are exactly ±1. Everything a simulation
//! measures in is scaled by a power of two — `I16F16` is 1/65536, `I24F8` is
//! 1/256 — so the two scales do not line up and crossing between them is a
//! multiply and a divide rather than a shift. This module is that crossing,
//! written once: it was hand-rolled bit arithmetic at every call site
//! otherwise, and a game that did not fancy writing it reached for a digital
//! action instead.

use corvid_fixed::{I16F16, I24F8, Signed16};

use corvid_vector::GlobalPoint;

use crate::Analog;
use corvid_vector::FinePoint;

/// One axis as a quantity, `full` at the positive end.
///
/// Exact at the three values a caller can name without measuring: `MAX` gives
/// `full`, `MIN` gives `-full`, and zero gives zero. Everything between is
/// `axis * full` **rounded to the nearest representable quantity**, and the
/// rounding is symmetric: `scale(-axis, full) == -scale(axis, full)` for every
/// axis and every scale.
///
/// Which way a halfway case goes is a question with no answer here, and saying
/// "half away from zero" would be describing a branch nothing reaches. A tie
/// needs `axis * full` to land exactly between two quantities, which needs
/// `2 * axis * full` to be an odd multiple of 32767 — and 32767 is odd, so it
/// would have to be a half-integer. The expression is written in the
/// away-from-zero form regardless, because that is the form that is symmetric
/// about zero for reasons the reader can check locally rather than by knowing
/// that the denominator is prime.
///
/// Integer arithmetic throughout — a widen to `i64`, a multiply, a rounded
/// divide by 32767 — so the answer is the same on every target. That matters
/// even though an axis is client-local: what an `intend` builds out of one is
/// an `Action`, and an action is hashed by every peer.
///
/// ```
/// use corvid_fixed::{I16F16, Signed16};
/// use corvid_input::scale;
///
/// let full = I16F16::from_f64(2.5);
///
/// // The ends are exact, which the obvious `bits >> 15` is not: that gives
/// // 32767/32768 of the scale at the top and never reaches it.
/// assert_eq!(scale(Signed16::MAX, full), full);
/// assert_eq!(scale(Signed16::MIN, full), -full);
/// assert_eq!(scale(Signed16::ZERO, full), I16F16::ZERO);
///
/// // The middle is the product, rounded once — and "the middle" is not a
/// // half, because an axis is `bits / 32767` and 32767 is odd. The nearest
/// // axis to a half is 16384 bits, which is a hair over, so the quantity is a
/// // hair over 1.25 and this is what "rounded once" looks like written down.
/// let half = Signed16::from_bits(16_384);
/// assert_eq!(scale(half, full).to_bits(), 81_923);
/// assert_eq!(I16F16::from_f64(1.25).to_bits(), 81_920);
///
/// // Whatever it rounds to, it rounds there in both directions.
/// assert_eq!(scale(-half, full), -scale(half, full));
/// ```
#[must_use]
pub const fn scale(axis: Signed16, full: I16F16) -> I16F16 {
    I16F16::from_bits(narrow(quantity(axis, full.to_bits() as i64)))
}

/// The same crossing onto the coarse tier, for a game whose quantities are
/// [`I24F8`].
///
/// Same rounding, same exactness at the ends. It is a separate function rather
/// than a generic because the two tiers are two types with two ranges, and a
/// caller that picked the wrong one should be told by the compiler rather than
/// by a saturating multiply.
///
/// ```
/// use corvid_fixed::{I24F8, Signed16};
/// use corvid_input::scale_coarse;
///
/// let full = I24F8::from_f64(100.0);
/// assert_eq!(scale_coarse(Signed16::MAX, full), full);
/// assert_eq!(scale_coarse(Signed16::MIN, full), -full);
/// ```
#[must_use]
pub const fn scale_coarse(axis: Signed16, full: I24F8) -> I24F8 {
    I24F8::from_bits(narrow(quantity(axis, full.to_bits() as i64)))
}

/// The quantity back in the storage a tier holds.
///
/// The product of a canonical axis and an `i32` scale, divided by 32767, has at
/// most the scale's own magnitude — an axis is at most one — so the third arm
/// is the only one that ever runs. The two clamps are there because "cannot
/// happen" and "wraps to a quantity of the opposite sign if it does" are a bad
/// pair to leave together, and because `TryFrom` is not available in a `const
/// fn`.
use corvid_bits::narrow_i64 as narrow;

/// The whole of the arithmetic, in the storage both tiers use.
///
/// `axis` is canonicalized first, so the `SNORM` denormal — the second bit
/// pattern for −1.0 — cannot produce a quantity one step outside the range.
/// The product of a canonical axis and an `i32` scale is below 2^46, so the
/// `i64` here cannot overflow and nothing saturates.
const fn quantity(axis: Signed16, scale: i64) -> i64 {
    let numerator = axis.canonicalize().to_bits() as i64 * scale;
    let denominator = Signed16::MAX.to_bits() as i64;
    // To nearest, with the doubling inside the numerator so the whole
    // expression stays in integers. Written away from zero because that is
    // visibly symmetric: an axis is a stick, and a rounding that leaned one way
    // — a floor, which is what `div_euclid` on the undoubled numerator is —
    // makes a push left one step smaller than the same push right, on every
    // frame, in an action every peer hashes. Half *up* would be symmetric too
    // here, but only because 32767 is odd and a tie therefore cannot happen,
    // which is a fact about the denominator rather than about this expression.
    if numerator >= 0 {
        (2 * numerator + denominator) / (2 * denominator)
    } else {
        -((-2 * numerator + denominator) / (2 * denominator))
    }
}

impl Analog {
    /// Both axes as an offset in the ground plane, `full` at the ends.
    ///
    /// **+X is right and +Y is forward**, which is this workspace's
    /// convention, so a stick pushed forward moves along +Y and the vertical
    /// component is zero. A game that wants a stick to drive height reads
    /// [`y`](Self::y) through [`scale`] itself and puts it where it likes;
    /// this is the mapping that is right often enough to have a name.
    ///
    /// ```
    /// use corvid_fixed::{I16F16, Signed16};
    /// use corvid_input::Analog;
    /// use corvid_vector::finepoint;
    ///
    /// let stick = Analog::new(Signed16::ZERO, Signed16::MAX);
    /// let metres_per_tick = I16F16::from_f64(0.25);
    ///
    /// assert_eq!(stick.on_the_ground(metres_per_tick), finepoint(0, metres_per_tick, 0));
    /// ```
    #[must_use]
    pub const fn on_the_ground(self, full: I16F16) -> FinePoint {
        FinePoint::new(scale(self.x, full), scale(self.y, full), I16F16::ZERO)
    }

    /// The same on the coarse tier, for a game whose positions are
    /// [`GlobalPoint`].
    #[must_use]
    pub const fn on_the_ground_coarse(self, full: I24F8) -> GlobalPoint {
        GlobalPoint::new(
            scale_coarse(self.x, full),
            scale_coarse(self.y, full),
            I24F8::ZERO,
        )
    }
}
