//! Projection onto a direction: the mixed-scale products a cast is built from.
//!
//! # Why these fit an `i64`
//!
//! A [`GlobalPoint`] component is an [`I24F8`] -- a Q8 `i32` -- and a
//! [`Direction`] component is a [`Signed32`], a Q31 `i32`. Their product is Q39
//! and reaches `2^62`, so three of them *summed* look as though they need more
//! than an `i64`: `3 * 2^62` is half again past `i64::MAX`.
//!
//! They do not, because a [`Direction`] is a **unit** vector and three maxed
//! components is not one. The sum is a dot product, so Cauchy-Schwarz bounds it
//! by the product of the lengths -- and the direction's length is exactly
//! [`UNIT`]:
//!
//! ```text
//! |offset . direction|  <=  |offset| * UNIT  <=  sqrt(3) * 2^31 * 2^31
//!                        =  7.99e18  <  9.22e18  =  i64::MAX
//! ```
//!
//! Thirteen percent of headroom, and the bound is *tight*: a direction down the
//! diagonal against the opposite corner of the world reaches it exactly.
//! `tests/project.rs` searches for that corner rather than taking the algebra's
//! word for it.
//!
//! Every product below is bounded the same way, by Cauchy-Schwarz against a
//! unit direction, and every one of them fits. Nothing here is wider than an
//! `i64`, and the two sums of squares that would be are taken unsigned, where
//! `3 * 2^62` still fits.
//!
//! A difference of two points is a [`GlobalPoint`] like any other, which is a
//! constraint rather than an observation: two points more than 8388 km apart
//! have no offset in this type, and
//! [`checked_sub`](GlobalPoint::checked_sub) is how a caller finds that out
//! rather than being handed a saturated one.
//!
//! # Why they divide by [`UNIT`] rather than shifting by 31
//!
//! A unit [`Signed32`] is `i32::MAX`, which is `2^31 - 1`. Shifting a product
//! right by 31 divides by one more than the scale it was multiplied by, and an
//! arithmetic shift floors -- so the sub-unit shortfall does not vanish, it
//! takes a whole step in the last place with it. Systematic, in the same
//! direction every time, and a cursor cast at the surface it is standing on
//! lands under it.

use corvid_fixed::{I24F8, Signed32};

use super::{Direction, GlobalPoint};

/// What a unit [`Signed32`] is, widened.
///
/// `i32::MAX`, and **not** `2^31`, for the reason the module documents.
const UNIT: i64 = i32::MAX as i64;

/// A quotient, rounded to nearest with halves away from zero.
///
/// Rust's integer division truncates toward zero, which turns every sub-unit
/// shortfall into a whole step in the last place. Every scaling here goes
/// through this instead.
#[inline]
pub(super) const fn divide(numerator: i64, denominator: i64) -> i64 {
    let half = (denominator.unsigned_abs() / 2) as i64;
    let bump = if numerator < 0 { -half } else { half };
    (numerator + bump) / denominator
}

impl GlobalPoint {
    /// How far along `direction` this offset reaches, in metres.
    ///
    /// Signed: an offset the other way answers a negative, and one across the
    /// direction answers zero. This is the function a sphere's `b` term, a
    /// plane's numerator and a triangle's barycentric coordinates are each one
    /// line of.
    ///
    /// ```
    /// use corvid_fixed::I24F8;
    /// use corvid_vector::{Direction, globalpoint};
    ///
    /// assert_eq!(globalpoint(0, 4, 0).project(Direction::Y), I24F8::from_f64(4.0));
    /// assert_eq!(globalpoint(4, 0, 0).project(Direction::Y), I24F8::ZERO);
    /// assert_eq!(globalpoint(0, -4, 0).project(Direction::Y), I24F8::from_f64(-4.0));
    /// ```
    #[must_use]
    #[inline]
    pub const fn project(self, direction: Direction) -> I24F8 {
        let [ox, oy, oz] = [self.0[0], self.0[1], self.0[2]];
        let [dx, dy, dz] = [direction.0[0], direction.0[1], direction.0[2]];
        // Q8 x Q31 = Q39; dividing by the unit takes it back to Q8. The sum
        // fits for the reason the module gives.
        let sum = (ox.to_bits() as i64) * (super::signed32_bits(dx) as i64)
            + (oy.to_bits() as i64) * (super::signed32_bits(dy) as i64)
            + (oz.to_bits() as i64) * (super::signed32_bits(dz) as i64);
        I24F8::saturating_from_bits(divide(sum, UNIT))
    }
}

impl Direction {
    /// How much two directions agree: `-1` opposite, `0` perpendicular, `1` the
    /// same.
    ///
    /// A caller that wants back-face culling compares this against zero.
    ///
    /// ```
    /// use corvid_fixed::Signed32;
    /// use corvid_vector::Direction;
    ///
    /// assert_eq!(Direction::Y.align(Direction::Y), Signed32::MAX);
    /// assert_eq!(Direction::Y.align(Direction::X), Signed32::ZERO);
    /// assert!(Direction::Y.align(-Direction::Y) < Signed32::ZERO);
    /// ```
    #[must_use]
    #[inline]
    pub const fn align(self, other: Direction) -> Signed32 {
        let [ax, ay, az] = [self.0[0], self.0[1], self.0[2]];
        let [bx, by, bz] = [other.0[0], other.0[1], other.0[2]];
        // Q31 x Q31 = Q62, and both operands are unit, so the sum is at most
        // `UNIT^2` -- comfortably inside an `i64`.
        let sum = (super::signed32_bits(ax) as i64) * (super::signed32_bits(bx) as i64)
            + (super::signed32_bits(ay) as i64) * (super::signed32_bits(by) as i64)
            + (super::signed32_bits(az) as i64) * (super::signed32_bits(bz) as i64);
        // The narrow takes the signed family's own wide type, which is one
        // step past the accumulator this needed.
        Signed32::saturating_from_bits(divide(sum, UNIT) as i128)
    }

    /// This direction walked `distance` metres.
    ///
    /// What a ray's `at` is, and what a hit's point is reconstructed with.
    ///
    /// ```
    /// use corvid_fixed::I24F8;
    /// use corvid_vector::{Direction, globalpoint};
    ///
    /// assert_eq!(Direction::Y.along(I24F8::from_f64(4.0)), globalpoint(0, 4, 0));
    /// ```
    #[must_use]
    #[inline]
    pub const fn along(self, distance: I24F8) -> GlobalPoint {
        GlobalPoint([
            scale(self.0[0], distance),
            scale(self.0[1], distance),
            scale(self.0[2], distance),
        ])
    }
}

/// One component of [`Direction::along`].
///
/// A free function rather than a closure, because a closure cannot be called
/// from a `const fn`. Q31 x Q8 = Q39, a single `i64` multiply that needs no
/// bound argument at all.
#[inline]
const fn scale(component: Signed32, distance: I24F8) -> I24F8 {
    let product = (super::signed32_bits(component) as i64) * (distance.to_bits() as i64);
    I24F8::saturating_from_bits(divide(product, UNIT))
}
