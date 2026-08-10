//! The offset between two points that are further apart than one can reach.
//!
//! [`GlobalPoint`]'s subtraction saturates each component, which for two points
//! at opposite ends of the world is not merely imprecise -- it clamps one axis
//! and not another, so what comes back points somewhere the input did not.
//! [`WideOffset`] is the difference taken before anything narrows, and it is
//! where every operation that starts from a pair of far-apart points begins.
//!
//! # How wide "wide" is
//!
//! Exactly one bit. A [`GlobalPoint`] component is an `i32` of Q8 bits, so a
//! difference of two of them spans `2^32` -- one more bit than a component, and
//! nowhere near a second word. Everything here is `i64` on that account, except
//! the operations that multiply two of these together: a cross product of two
//! offsets reaches `2^65` and a [`Volume`](super::Volume) of three reaches
//! `2^98`, and each says so where it is written.
//!
//! # What it answers
//!
//! Ordinary types. [`half`](WideOffset::half) and
//! [`narrow`](WideOffset::narrow) come back as a [`GlobalPoint`],
//! [`project`](WideOffset::project) as an [`I24F8`], the squared lengths as an
//! [`I48F16`] at the doubled scale, and the directions as a [`Direction`]. A
//! caller never sees a bit pattern, which is the point: the widening is a
//! property of the arithmetic rather than of the geometry, and geometry is what
//! the caller has.

use corvid_fixed::{I24F8, I48F16};

use super::{Direction, GlobalPoint};

/// What a unit [`Signed32`](corvid_fixed::Signed32) is, widened.
const UNIT: i64 = i32::MAX as i64;

/// The offset between two [`GlobalPoint`]s, before anything narrows.
///
/// ```
/// use corvid_fixed::I24F8;
/// use corvid_vector::{Direction, WideOffset, globalpoint};
///
/// // Two points 16 000 km apart, which is twice what a component reaches.
/// let far = globalpoint(I24F8::from_f64(8e6), I24F8::ZERO, I24F8::ZERO);
/// let offset = WideOffset::between(far, -far);
///
/// // The difference does not fit a point, so narrowing it clamps ...
/// assert_eq!(offset.narrow(), globalpoint(I24F8::MAX, I24F8::ZERO, I24F8::ZERO));
///
/// // ... but halving it, projecting it and taking its direction all answer
/// // the geometry rather than the clamp.
/// assert_eq!(offset.half(), far);
/// assert_eq!(offset.direction(), Some(Direction::X));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WideOffset(pub(super) [i64; 3]);

impl WideOffset {
    /// The offset from `from` to `to`.
    ///
    /// The reason this exists rather than `to - from`: that subtraction
    /// saturates each component independently, so a difference wider than one
    /// component's range is clamped on one axis and not the next, and the
    /// answer is a different bearing rather than a shorter one.
    #[must_use]
    #[inline]
    pub const fn between(to: GlobalPoint, from: GlobalPoint) -> Self {
        let [tx, ty, tz] = to.to_array();
        let [fx, fy, fz] = from.to_array();
        Self([
            tx.to_bits() as i64 - fx.to_bits() as i64,
            ty.to_bits() as i64 - fy.to_bits() as i64,
            tz.to_bits() as i64 - fz.to_bits() as i64,
        ])
    }

    /// This offset as a [`GlobalPoint`], clamping each component.
    ///
    /// For the caller who has established that the offset is in range, or who
    /// wants the clamp.
    #[must_use]
    #[inline]
    pub const fn narrow(self) -> GlobalPoint {
        GlobalPoint::new(
            I24F8::saturating_from_bits(self.0[0]),
            I24F8::saturating_from_bits(self.0[1]),
            I24F8::saturating_from_bits(self.0[2]),
        )
    }

    /// Half of this offset, which always fits a [`GlobalPoint`].
    ///
    /// Half the difference of two points is at most half the range apart from
    /// either, so this is the widening earning its keep: the halving happens
    /// before the narrowing, and a box 16 000 km across still knows where its
    /// middle is.
    #[must_use]
    #[inline]
    pub const fn half(self) -> GlobalPoint {
        GlobalPoint::new(
            I24F8::saturating_from_bits(self.0[0] / 2),
            I24F8::saturating_from_bits(self.0[1] / 2),
            I24F8::saturating_from_bits(self.0[2] / 2),
        )
    }

    /// Whether the two points coincide.
    #[must_use]
    #[inline]
    pub const fn is_zero(self) -> bool {
        self.0[0] == 0 && self.0[1] == 0 && self.0[2] == 0
    }

    /// How far along `direction` this offset reaches, in metres.
    ///
    /// [`GlobalPoint::project`] for an offset that may be wider than a point,
    /// and exact in the same way -- though it has to work for it. That version
    /// sums three products and divides once, which it can because a unit
    /// direction bounds the sum; here the offset carries a further bit and the
    /// sum does not fit. So each product is divided on its own and the three
    /// remainders are carried and divided together, which is the same
    /// arithmetic in a different order and answers the same integer.
    ///
    /// The answer clamps when the offset is longer than a point can express,
    /// which is a distance no [`I24F8`] was going to hold either.
    #[must_use]
    #[inline]
    pub const fn project(self, direction: Direction) -> I24F8 {
        let [dx, dy, dz] = direction.to_array();
        let mut whole = 0;
        let mut rest = 0;
        // Q8 x Q31 = Q39, at most `2^63 - 2^32 - 2^31 + 1`, which is an `i64`
        // by a hair. Dividing by the unit takes each term back to Q8.
        let (quotient, remainder) = split(self.0[0] * super::signed32_bits(dx) as i64);
        whole += quotient;
        rest += remainder;
        let (quotient, remainder) = split(self.0[1] * super::signed32_bits(dy) as i64);
        whole += quotient;
        rest += remainder;
        let (quotient, remainder) = split(self.0[2] * super::signed32_bits(dz) as i64);
        whole += quotient;
        rest += remainder;
        I24F8::saturating_from_bits(whole + divide(rest, UNIT))
    }

    /// The squared length, at the doubled scale.
    ///
    /// Squared because comparing distances needs no square root, and at the
    /// doubled scale because that is what squaring does to a scale: an
    /// [`I48F16`] is the Q16 an [`I24F8`] squares into, and
    /// [`root`](I48F16::root) takes it back.
    ///
    /// Saturates past `1.4e14` square metres, which is a separation of about
    /// 11 800 km. A radius is an [`I24F8`], so it squares to at most half of
    /// what saturation answers -- which is why a comparison against one stays
    /// correct on the far side of the clamp.
    #[must_use]
    #[inline]
    pub const fn length_squared(self) -> I48F16 {
        let mut total = 0u64;
        let mut axis = 0;
        while axis < 3 {
            let magnitude = self.0[axis].unsigned_abs();
            total = total.saturating_add(magnitude.saturating_mul(magnitude));
            axis += 1;
        }
        I48F16::saturating_from_bits(total as i128)
    }

    /// How far this offset lies from the line through `direction`, squared, at
    /// the doubled scale.
    ///
    /// The other half of [`project`](Self::project): what the projection leaves
    /// behind, which for a ray is how close it passes to the origin of the
    /// offset. A cast at a sphere is this against the squared radius and
    /// nothing else, and taking it directly is what keeps that comparison exact
    /// for a ray that starts on the far side of the world -- `|offset|^2` and
    /// the squared projection are each near `2^64` there and their difference
    /// is small, so subtracting them after the fact would be subtracting two
    /// large numbers to get a small one.
    ///
    /// This is the one operation here that needs a wider accumulator than an
    /// `i64`, and it needs it twice over: the cross product it is built on has
    /// components reaching `2^64`, and their squares reach `2^68` even after
    /// the unit is divided out. Lagrange's identity is what makes it a cross
    /// product rather than a difference of two squares:
    ///
    /// ```text
    /// |offset|^2 |direction|^2 - (offset . direction)^2  =  |offset x direction|^2
    /// ```
    ///
    /// Saturates where [`length_squared`](Self::length_squared) does, and for
    /// the same reason it stays a correct comparison.
    #[must_use]
    #[inline]
    pub const fn rejection_squared(self, direction: Direction) -> I48F16 {
        let [dx, dy, dz] = direction.to_array();
        let unit = [
            super::signed32_bits(dx) as i128,
            super::signed32_bits(dy) as i128,
            super::signed32_bits(dz) as i128,
        ];
        let wide = [self.0[0] as i128, self.0[1] as i128, self.0[2] as i128];
        let mut total = 0i128;
        let mut axis = 0;
        while axis < 3 {
            let (next, after) = ((axis + 1) % 3, (axis + 2) % 3);
            // Q8 x Q31 = Q39; dividing by the unit takes the component of the
            // cross back to Q8, which is where its square is the Q16 the answer
            // wants. Dividing before squaring is also what keeps the square
            // inside an `i128`.
            let term = divide_wide(wide[next] * unit[after] - wide[after] * unit[next]);
            total += term * term;
            axis += 1;
        }
        I48F16::saturating_from_bits(total)
    }

    /// The unit direction along this offset, or [`None`] if it is zero.
    #[must_use]
    #[inline]
    pub const fn direction(self) -> Option<Direction> {
        super::normalize_bits(
            [self.0[0] as i128, self.0[1] as i128, self.0[2] as i128],
            false,
        )
    }

    /// The unit direction perpendicular to both, or [`None`] if they are
    /// parallel.
    ///
    /// A triangle's normal, from its two edges. The cross product itself has
    /// components reaching `2^65`, which is why it is not a [`WideOffset`] --
    /// but only its ratios are wanted, so it is normalized where it is computed
    /// and never named.
    #[must_use]
    #[inline]
    pub const fn cross_direction(self, other: Self) -> Option<Direction> {
        let [ax, ay, az] = [self.0[0] as i128, self.0[1] as i128, self.0[2] as i128];
        let [bx, by, bz] = [other.0[0] as i128, other.0[1] as i128, other.0[2] as i128];
        super::normalize_bits(
            [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx],
            false,
        )
    }
}

/// A product split into whole units and a remainder, so that three of them can
/// be summed and rounded once.
///
/// Rust's `/` truncates toward zero and `%` takes the numerator's sign, so
/// `quotient * UNIT + remainder` is the input exactly and the remainders can be
/// carried. Three of them stay under `3 * UNIT`, and the unit is odd -- so a
/// half never lands exactly on one and the rounding of the sum is the rounding
/// of the whole.
#[inline]
const fn split(product: i64) -> (i64, i64) {
    (product / UNIT, product % UNIT)
}

/// A component of a cross product brought back to Q8, rounded to nearest.
///
/// One width up, and by the unit rather than by a shift: see
/// [`divide`] for both reasons.
#[inline]
const fn divide_wide(numerator: i128) -> i128 {
    let denominator = UNIT as i128;
    let half = (denominator.unsigned_abs() / 2).cast_signed();
    let bump = if numerator < 0 { -half } else { half };
    (numerator + bump) / denominator
}

/// A quotient, rounded to nearest with halves away from zero.
///
/// Rust's integer division truncates toward zero, which turns every sub-unit
/// shortfall into a whole step in the last place. Systematic, in the same
/// direction every time, and a cursor cast at the surface it is standing on
/// lands under it.
#[inline]
const fn divide(numerator: i64, denominator: i64) -> i64 {
    let half = (denominator.unsigned_abs() / 2).cast_signed();
    let bump = if numerator < 0 { -half } else { half };
    (numerator + bump) / denominator
}
