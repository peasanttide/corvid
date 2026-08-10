//! Squared distances, and the barycentric determinants a mesh pick is.
//!
//! Split from [`project`](super::project) because a file stays under 400 lines,
//! and this is the seam that was already there: everything there answers a
//! length along a direction, and everything here answers an area or a square.
//!
//! # Why the squares are unsigned and the determinants are not
//!
//! A sum of three squared [`I24F8`](corvid_fixed::I24F8) bit patterns reaches `3 * 2^62`, which is
//! half again past `i64::MAX` and comfortably inside `u64`. A square has no
//! sign to lose, so the unsigned width is free -- and the results narrow into
//! an [`I48F16`], the Q16 an [`I24F8`](corvid_fixed::I24F8) squares into, which saturates at
//! `1.4e14` square metres. Every radius this workspace has squares to less than
//! half of that, so a comparison against one stays an answer on both sides of
//! the clamp.
//!
//! A determinant is signed and cannot take that route, so it is bounded rather
//! than widened: see [`Volume`].

use corvid_fixed::{Factor32, I48F16};

use super::{Direction, GlobalPoint};

/// What a unit [`Signed32`](corvid_fixed::Signed32) is, widened.
const UNIT: i64 = i32::MAX as i64;

impl GlobalPoint {
    /// The squared distance to another point, at the doubled scale.
    ///
    /// Squared because comparing distances needs no square root, and at the
    /// doubled scale because that is what squaring does to a scale: an
    /// [`I48F16`] is the Q16 an [`I24F8`](corvid_fixed::I24F8) squares into, and
    /// [`root`](I48F16::root) takes it back.
    ///
    /// Two points further apart than an [`I24F8`](corvid_fixed::I24F8) reaches answer
    /// [`I48F16::MAX`], which is the honest reading -- it is further than this
    /// type can express, and further than any radius, so a comparison against
    /// one is still right. The difference is taken with
    /// [`checked_sub`](Self::checked_sub) rather than the saturating
    /// subtraction for that reason: a clamped difference would answer a
    /// *shorter* distance than the truth, which is the one direction that turns
    /// a miss into a hit.
    ///
    /// ```
    /// use corvid_fixed::{I24F8, I48F16};
    /// use corvid_vector::{GlobalPoint, globalpoint};
    ///
    /// assert_eq!(globalpoint(3, 4, 0).distance_squared(GlobalPoint::ZERO), I48F16::from(25));
    ///
    /// // Further apart than the type reaches, so as far as it can say.
    /// let edge = globalpoint(I24F8::MAX, I24F8::ZERO, I24F8::ZERO);
    /// assert_eq!(edge.distance_squared(-edge), I48F16::MAX);
    /// ```
    #[must_use]
    #[inline]
    pub const fn distance_squared(self, other: Self) -> I48F16 {
        let Some(offset) = self.checked_sub(other) else {
            return I48F16::MAX;
        };
        square_sum([
            offset.0[0].to_bits() as i64,
            offset.0[1].to_bits() as i64,
            offset.0[2].to_bits() as i64,
        ])
    }

    /// How far this offset lies from the line through `direction`, squared, at
    /// the doubled scale.
    ///
    /// The other half of [`project`](Self::project): what the projection leaves
    /// behind, which for a ray is how close it passes. A cast at a sphere is
    /// this against the squared radius and nothing else.
    ///
    /// Taken directly rather than as `|self|^2 - project^2`, which is the same
    /// number and a much worse way to reach it: both of those terms are near
    /// `2^62` for an offset at the edge of the range and their difference is
    /// the small quantity that decides hit from miss. Lagrange's identity makes
    /// it a cross product instead, where the smallness is in the operands:
    ///
    /// ```text
    /// |self|^2 |direction|^2 - (self . direction)^2  =  |self x direction|^2
    /// ```
    ///
    /// ```
    /// use corvid_fixed::I48F16;
    /// use corvid_vector::{Direction, globalpoint};
    ///
    /// let offset = globalpoint(0, 4, 0);
    /// assert_eq!(offset.rejection_squared(Direction::Y), I48F16::ZERO);
    /// assert_eq!(offset.rejection_squared(Direction::X), I48F16::from(16));
    /// ```
    #[must_use]
    #[inline]
    pub const fn rejection_squared(self, direction: Direction) -> I48F16 {
        let cross = self.cross_bits(direction);
        // Each component of the cross is Q39; dividing by the unit takes it to
        // the Q8 whose square is the Q16 wanted.
        square_sum([
            super::project::divide(cross[0], UNIT),
            super::project::divide(cross[1], UNIT),
            super::project::divide(cross[2], UNIT),
        ])
    }

    /// The unit direction perpendicular to both, or [`None`] if they are
    /// parallel.
    ///
    /// A triangle's normal, from its two edges. This is what
    /// [`from_ratio`](Direction::from_ratio) is for and why it takes an `i64`:
    /// the cross product of two [`GlobalPoint`]s is a product of two Q8 bit
    /// patterns and reaches `2^62`, so it fits a word and fits nothing
    /// narrower. Going through [`cross`](Self::cross) instead would divide it
    /// back to a component's range first, and a normal recovered from three
    /// saturated components points somewhere the face does not.
    ///
    /// ```
    /// use corvid_vector::{Direction, globalpoint};
    ///
    /// assert_eq!(globalpoint(2, 0, 0).cross_direction(globalpoint(0, 3, 0)), Some(Direction::Z));
    /// assert_eq!(globalpoint(2, 0, 0).cross_direction(globalpoint(4, 0, 0)), None);
    /// ```
    #[must_use]
    #[inline]
    pub const fn cross_direction(self, other: Self) -> Option<Direction> {
        let [ax, ay, az] = [
            self.0[0].to_bits() as i64,
            self.0[1].to_bits() as i64,
            self.0[2].to_bits() as i64,
        ];
        let [bx, by, bz] = [
            other.0[0].to_bits() as i64,
            other.0[1].to_bits() as i64,
            other.0[2].to_bits() as i64,
        ];
        Direction::from_ratio([ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx])
    }

    /// The signed volume of the box this, a direction and another offset span:
    /// `self . (direction x other)`.
    ///
    /// The quantity Moller-Trumbore is four of. See [`Volume`] for what can be
    /// asked of it and why it is not a number.
    #[must_use]
    #[inline]
    pub const fn volume(self, direction: Direction, other: Self) -> Volume {
        let cross = other.cross_bits(direction);
        // Halved on the way past, which is what keeps the sum below inside a
        // word: without it the bound is `3 * 2^62`, half again too much. The
        // scale that leaves is arbitrary and shared, which is all a comparison
        // of two of these and a ratio between them need.
        let scaled = [
            super::project::divide(cross[0], 2 * UNIT),
            super::project::divide(cross[1], 2 * UNIT),
            super::project::divide(cross[2], 2 * UNIT),
        ];
        Volume(
            (self.0[0].to_bits() as i64) * scaled[0]
                + (self.0[1].to_bits() as i64) * scaled[1]
                + (self.0[2].to_bits() as i64) * scaled[2],
        )
    }

    /// `direction x self`, as three Q39 bit patterns.
    ///
    /// Each is a two-by-two minor, so Cauchy-Schwarz bounds it by
    /// `sqrt(2) * 2^31 * 2^31` rather than by the `2^63` that two maxed
    /// products suggest -- 30% inside a word. Only the magnitude matters to
    /// [`rejection_squared`](Self::rejection_squared); the order is the one
    /// [`volume`](Self::volume) wants.
    #[inline]
    const fn cross_bits(self, direction: Direction) -> [i64; 3] {
        let [ax, ay, az] = [
            super::signed32_bits(direction.0[0]) as i64,
            super::signed32_bits(direction.0[1]) as i64,
            super::signed32_bits(direction.0[2]) as i64,
        ];
        let [bx, by, bz] = [
            self.0[0].to_bits() as i64,
            self.0[1].to_bits() as i64,
            self.0[2].to_bits() as i64,
        ];
        [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
    }
}

/// The sum of three squared Q8 patterns, as a Q16, saturating.
///
/// Unsigned because a square has no sign to lose and `3 * 2^62` fits a `u64`
/// where it does not fit an `i64`.
#[inline]
const fn square_sum(bits: [i64; 3]) -> I48F16 {
    let mut total = 0u64;
    let mut axis = 0;
    while axis < 3 {
        let magnitude = bits[axis].unsigned_abs();
        total = total.saturating_add(magnitude.saturating_mul(magnitude));
        axis += 1;
    }
    // Clamped in the unsigned width rather than handed to the type's own
    // saturating narrow, which would take one step wider than anything here
    // needs to go.
    let clamped = if total > i64::MAX as u64 {
        i64::MAX
    } else {
        total as i64
    };
    I48F16::from_bits(clamped)
}

/// The signed volume of a box three offsets span.
///
/// Opaque, and the scale it carries is deliberately not named: it is an area in
/// square metres times an arbitrary shared factor, and every question below is
/// scale-free. That is what lets the barycentric tests be comparisons rather
/// than divisions -- a division that rounded would let a ray through the seam
/// between two triangles that share an edge, which is a hole in a mesh that
/// appears at one pixel and never reproduces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Volume(i64);

impl Volume {
    /// Three offsets that lie in a plane.
    pub const ZERO: Self = Self(0);

    /// Whether the three lie in a plane, which for a triangle means it is
    /// degenerate and for a ray means it is parallel to the face.
    #[must_use]
    #[inline]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Two added, for a barycentric coordinate that is the sum of two.
    #[must_use]
    #[inline]
    pub const fn add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// Whether this falls outside `0 ..= bound`, with `bound`'s sign folded in.
    #[must_use]
    #[inline]
    pub const fn is_outside(self, bound: Self) -> bool {
        if bound.0 > 0 {
            self.0 < 0 || self.0 > bound.0
        } else {
            self.0 > 0 || self.0 < bound.0
        }
    }

    /// `self / bound`, for a `self` already known to lie between zero and it.
    ///
    /// The one division a mesh pick does, and it is done last: everything that
    /// decides hit from miss is a comparison, and this only places the hit once
    /// the answer is yes. [`Factor32::MAX`] for a zero bound, which
    /// [`is_zero`](Self::is_zero) has already ruled out at every call site.
    #[must_use]
    #[inline]
    pub const fn ratio(self, bound: Self) -> Factor32 {
        let denominator = bound.0.unsigned_abs();
        if denominator == 0 {
            return Factor32::MAX;
        }
        let numerator = self.0.unsigned_abs();
        if numerator >= denominator {
            return Factor32::MAX;
        }
        // The quotient is under one by construction, so what is left is to
        // reach a Q32 of it without a wider multiply than a word. Both sides
        // shift down until the denominator fits 32 bits, which loses only bits
        // the answer could not have shown anyway.
        let shift = (64 - denominator.leading_zeros()).saturating_sub(32);
        let scaled = (numerator >> shift) * (Factor32::MAX.to_bits() as u64);
        Factor32::from_bits((scaled / (denominator >> shift)) as u32)
    }
}
