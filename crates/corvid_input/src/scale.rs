//! Turning an axis into a quantity.
//!
//! An [`Analog`](crate::Analog) axis is a [`Signed16`]: `SNORM`, so the value
//! is `bits / 32767` and the ends are exactly +/-1. Everything a simulation
//! measures in is scaled by a power of two -- `I16F16` is 1/65536, `I24F8` is
//! 1/256 -- so the two scales do not line up and crossing between them is a
//! multiply and a rounded divide rather than a shift.
//!
//! The arithmetic itself is [`I16F16::saturating_mul_signed16`] and its coarse
//! twin, in `corvid_fixed`, which is where a crossing between two fixed-point
//! scales belongs: the rounding, the symmetry about zero and the one corner
//! where a two's-complement range has no negation are properties of the
//! scalars rather than of input. What is here is the naming -- an axis, a full
//! deflection, and the ground plane -- and the axis convention that only a
//! game knows.

use corvid_fixed::{I16F16, I24F8, Signed16};

use corvid_vector::{FinePoint, GlobalPoint};

use crate::Analog;

/// One axis as a quantity, `full` at the positive end.
///
/// Exactly [`I16F16::saturating_mul_signed16`], named for what it is here: the
/// axis is how far the control is pushed and `full` is what a whole push is
/// worth. See that method for the rounding and for the one scale whose
/// negation is not a value.
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
/// // The middle is the product, rounded once -- and "the middle" is not a
/// // half, because an axis is `bits / 32767` and 32767 is odd. The nearest
/// // axis to a half is 16384 bits, which is a hair over, so the quantity is a
/// // hair over 1.25 and this is what "rounded once" looks like written down.
/// let half = Signed16::from_bits(16_384);
/// assert_eq!(scale(half, full).to_bits(), 81_923);
/// assert_eq!(I16F16::from_f64(1.25).to_bits(), 81_920);
/// ```
#[must_use]
pub const fn scale(axis: Signed16, full: I16F16) -> I16F16 {
    full.saturating_mul_signed16(axis)
}

/// The same crossing onto the coarse tier, for a game whose quantities are
/// [`I24F8`].
///
/// A separate function rather than a generic because the two tiers are two
/// types with two ranges, and a caller that picked the wrong one should be told
/// by the compiler rather than by a saturating multiply.
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
    full.saturating_mul_signed16(axis)
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
