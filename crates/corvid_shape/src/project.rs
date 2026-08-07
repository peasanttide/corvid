//! The mixed-width dot products every cast in this crate is built from.
//!
//! # Why they all accumulate in `i128`
//!
//! A [`GlobalPoint`] component is an [`I24F8`] — a Q8 `i32`, reaching ±8388 km
//! at 3.9 mm — and a [`Direction`] component is a [`Signed32`], a Q31 `i32`.
//! Their product is Q39 and reaches 2⁶², so **three of them summed do not fit
//! an `i64`**: 3 × 2⁶² is a half more than `i64::MAX`. That is not a
//! theoretical bound either. It is reached by a ray cast at a point near the
//! edge of the world along a diagonal, which is a cursor pointed at the horizon
//! from the far side of a planet.
//!
//! `corvid_vector::dot` made the same choice for the same reason. Accumulating
//! wide and narrowing once, with a saturation rather than a wrap, is the shape
//! every function here has.
//!
//! # Why they divide by [`UNIT`] rather than shifting by 31
//!
//! Because a unit [`Signed32`] is `i32::MAX`, which is 2³¹ − **1**. Shifting a
//! product right by 31 therefore divides by one more than the scale it was
//! multiplied by, and an arithmetic shift floors — so the sub-unit shortfall
//! that leaves does not vanish, it takes a whole step in the last place with
//! it. That error is small enough to look like rounding and is not: it is
//! systematic, it is in the same direction every time, and a cursor cast at a
//! surface it is standing on lands under it.
//!
//! Dividing by [`UNIT`] and rounding to nearest makes every axis-aligned case
//! exact instead, which the doctests below and `tests/cast.rs` are written
//! against.

use corvid_fixed::{I24F8, Signed32};

use corvid_vector::{Direction, GlobalPoint};
/// What a unit [`Signed32`] is, as a wide integer.
///
/// `i32::MAX`, and **not** 2³¹. Every scaling in this crate divides by this
/// rather than shifting, for the reason the module's own documentation gives.
pub(crate) const UNIT: i128 = i32::MAX as i128;

/// How far along `direction` an offset reaches: `offset · direction`, in
/// metres.
///
/// Signed: an offset the other way answers a negative, and an offset across the
/// direction answers zero. This is the function a sphere's `b` term, a plane's
/// numerator and a triangle's barycentric coordinates are each one line of.
///
/// ```
/// use corvid_shape::project;
/// use corvid_fixed::I24F8;
/// use corvid_vector::{Direction, globalpoint};
///
/// assert_eq!(project(globalpoint(0, 4, 0), Direction::Y), I24F8::from_f64(4.0));
/// assert_eq!(project(globalpoint(4, 0, 0), Direction::Y), I24F8::ZERO);
/// assert_eq!(project(globalpoint(0, -4, 0), Direction::Y), I24F8::from_f64(-4.0));
/// ```
#[must_use]
pub fn project(offset: GlobalPoint, direction: Direction) -> I24F8 {
    let [ox, oy, oz] = offset.to_array();
    let [dx, dy, dz] = direction.to_array();
    // Q8 x Q31 = Q39; dividing by the unit takes it back to Q8.
    let sum = i128::from(ox.to_bits()) * i128::from(dx.to_bits())
        + i128::from(oy.to_bits()) * i128::from(dy.to_bits())
        + i128::from(oz.to_bits()) * i128::from(dz.to_bits());
    I24F8::from_bits(narrow(divide(sum, UNIT)))
}

/// How much two directions agree: `a · b`, from −1 to 1.
///
/// One for the same direction, zero for perpendicular, minus one for opposite.
/// A caller that wants back-face culling — which [`Triangle`](crate::Triangle)
/// deliberately does not do for it — compares this against zero.
///
/// ```
/// use corvid_shape::align;
/// use corvid_fixed::Signed32;
/// use corvid_vector::Direction;
///
/// assert_eq!(align(Direction::Y, Direction::Y), Signed32::MAX);
/// assert_eq!(align(Direction::Y, Direction::X), Signed32::ZERO);
/// assert!(align(Direction::Y, -Direction::Y) < Signed32::ZERO);
/// ```
#[must_use]
pub fn align(a: Direction, b: Direction) -> Signed32 {
    let [ax, ay, az] = a.to_array();
    let [bx, by, bz] = b.to_array();
    // Q31 x Q31 = Q62; dividing by the unit takes it back to Q31.
    let sum = i128::from(ax.to_bits()) * i128::from(bx.to_bits())
        + i128::from(ay.to_bits()) * i128::from(by.to_bits())
        + i128::from(az.to_bits()) * i128::from(bz.to_bits());
    Signed32::from_bits(narrow(divide(sum, UNIT)))
}

/// A length along a direction: `direction × distance`, as an offset.
///
/// What [`Ray::at`](crate::Ray::at) walks by, and what a hit's point is
/// reconstructed with.
#[must_use]
pub(crate) fn along(direction: Direction, distance: I24F8) -> GlobalPoint {
    let scaled = |component: Signed32| {
        // Q31 x Q8 = Q39; dividing by the unit takes it back to Q8.
        let product = i128::from(component.to_bits()) * i128::from(distance.to_bits());
        I24F8::from_bits(narrow(divide(product, UNIT)))
    };
    let [x, y, z] = direction.to_array();
    GlobalPoint::from_array([scaled(x), scaled(y), scaled(z)])
}

/// A cross product of two wide triples, at the sum of their scales.
///
/// Used by [`Triangle`](crate::Triangle) alone, where the operands are at two
/// different scales and neither may be narrowed on the way — which is why this
/// takes bit patterns rather than points, and why it has no return type that
/// says what it means.
#[must_use]
#[inline]
pub(crate) const fn cross_wide(a: [i128; 3], b: [i128; 3]) -> [i128; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A quotient, rounded to nearest with halves away from zero.
///
/// Rust's integer division truncates towards zero, which on its own turns every
/// sub-unit shortfall into a whole step in the last place — the failure the
/// module's own documentation describes. Every scaling and every ratio in this
/// crate goes through here instead.
///
/// The caller is responsible for a non-zero denominator. Every call site above
/// passes [`UNIT`]; every other one has already tested for the zero and
/// answered a miss, because a zero denominator in a cast is a ray parallel to
/// the thing it was cast at rather than an arithmetic accident.
#[must_use]
#[inline]
pub(crate) const fn divide(numerator: i128, denominator: i128) -> i128 {
    // `unsigned_abs` rather than `abs`, which overflows on `i128::MIN` — a
    // value no call site here can reach, and a panic the workspace forbids
    // being one branch away from is not worth the shorter spelling.
    let half = (denominator.unsigned_abs() / 2).cast_signed();
    let bump = if numerator < 0 { -half } else { half };
    (numerator + bump) / denominator
}

/// A wide accumulator brought back to a component, clamping rather than
/// wrapping.
///
/// Every narrowing in this crate goes through here. A cast that saturates
/// answers a hit at the far edge of the near field, which is wrong by a
/// distance no player can see; one that wrapped would answer a hit *behind* the
/// eye, which is a build cursor that jumps to the other side of the world.
pub(crate) use corvid_bits::narrow_i128 as narrow;
