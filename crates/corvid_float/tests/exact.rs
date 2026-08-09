//! The operations held to the intrinsic's bits.
//!
//! A square root, the four roundings, the two sign operations and the two
//! clamps each have a single right answer that IEEE 754 names, and both this
//! crate's software version and the platform's intrinsic reach it. So the
//! assertion is equality of bit patterns, including which zero and which
//! infinity, rather than a tolerance -- a tolerance here would hide the one
//! thing these tests exist to catch.

#![allow(
    clippy::float_cmp,
    reason = "exact equality is the assertion: these are bit-for-bit operations, and a tolerance would hide the one thing these tests exist to catch"
)]
#![allow(
    clippy::cast_precision_loss,
    reason = "the loop counters are small integers being turned into the sample points, which is the standard way to sweep a range and is exact for every value they take"
)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "one test deliberately spells out the `+0.5`-then-truncate rounding that a const conversion reaches for when it cannot call `wide::round`, and the truncation is the behaviour under test"
)]

mod common;

use common::{STRIDE, same, same_wide};

/// Bit for bit, and over bit patterns rather than over a range of values.
///
/// A square root is one of the five operations IEEE 754 requires to be
/// correctly rounded, so there is an exact answer to hold this to rather than a
/// tolerance -- and a tolerance would let a last-bit regression through in the
/// function the composed ones are all built out of. Sweeping bit patterns is
/// what reaches the subnormals and `MAX`; stepping by a sixteenth, as an
/// earlier version of this test did, never leaves a handful of exponents.
#[test]
fn square_roots_are_bit_for_bit_the_intrinsic() {
    for bits in (0..=u32::MAX).step_by(STRIDE) {
        let x = f32::from_bits(bits);
        same(corvid_float::sqrt(x), x.sqrt(), "sqrt");
    }
}

/// The documented behaviour at and below zero, which the sweep above reaches
/// only by accident and which the doc comment states outright.
#[test]
fn a_square_root_of_a_negative_is_a_nan_and_of_a_negative_zero_is_a_negative_zero() {
    assert!(corvid_float::sqrt(-1.0).is_nan());
    assert!(corvid_float::sqrt(f32::MIN).is_nan());
    assert!(corvid_float::sqrt(f32::NEG_INFINITY).is_nan());
    assert!(corvid_float::sqrt(f32::NAN).is_nan());
    same(corvid_float::sqrt(-0.0), (-0.0f32).sqrt(), "sqrt(-0.0)");
    assert!(corvid_float::sqrt(-0.0).is_sign_negative());
    same(corvid_float::sqrt(0.0), 0.0, "sqrt(0.0)");
    same(
        corvid_float::sqrt(f32::INFINITY),
        f32::INFINITY,
        "sqrt(inf)",
    );
}

/// The four roundings and the two sign operations, over the same sweep of bit
/// patterns and to the same bit-for-bit standard.
///
/// This is the test that says the composed [`corvid_float::ceil`] -- `-floor(-x)`
/// -- is the ceiling rather than an approximation of it, and that the roundings
/// have not quietly lost a sign on a zero. It is also where a `-0.0` regression
/// would show, which an `assert_eq!` between two floats cannot see: `-0.0 ==
/// 0.0` is true and their bits are not.
#[test]
fn the_exact_operations_are_bit_for_bit_the_intrinsics() {
    for bits in (0..=u32::MAX).step_by(STRIDE) {
        let x = f32::from_bits(bits);
        same(corvid_float::floor(x), x.floor(), "floor");
        same(corvid_float::ceil(x), x.ceil(), "ceil");
        same(corvid_float::trunc(x), x.trunc(), "trunc");
        same(corvid_float::round(x), x.round(), "round");
        same(corvid_float::abs(x), x.abs(), "abs");
        same(
            corvid_float::copysign(x, -1.0),
            x.copysign(-1.0),
            "copysign-",
        );
        same(corvid_float::copysign(x, 1.0), x.copysign(1.0), "copysign+");
    }
}

/// The values a strided sweep can miss, named rather than sampled.
#[test]
fn the_specials_are_the_intrinsics_specials() {
    let specials = [
        0.0_f32,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        f32::MIN_POSITIVE,
        -f32::MIN_POSITIVE,
        f32::from_bits(1),
        f32::from_bits(0x8000_0001),
        f32::MAX,
        f32::MIN,
        f32::from_bits(0.5_f32.to_bits() - 1),
    ];
    for x in specials {
        same(corvid_float::floor(x), x.floor(), "floor");
        same(corvid_float::ceil(x), x.ceil(), "ceil");
        same(corvid_float::trunc(x), x.trunc(), "trunc");
        same(corvid_float::round(x), x.round(), "round");
        same(corvid_float::abs(x), x.abs(), "abs");
        same(corvid_float::sqrt(x), x.sqrt(), "sqrt");
        same(
            corvid_float::copysign(x, -2.0),
            x.copysign(-2.0),
            "copysign",
        );
    }
}

/// The doc comment for [`corvid_float::abs`] says the sign-bit spelling is what
/// makes it right for `-0.0` and for `NaN`, and both of those are exactly the
/// values a comparison-based `abs` gets wrong. Neither survives an
/// `assert_eq!`, so both are checked on their bits.
#[test]
fn the_magnitude_is_a_sign_bit_and_not_a_comparison() {
    assert_eq!(corvid_float::abs(-0.0).to_bits(), 0.0_f32.to_bits());
    assert_eq!(corvid_float::abs(0.0).to_bits(), 0.0_f32.to_bits());
    let negative_nan = f32::from_bits(0xffc0_0000);
    assert!(corvid_float::abs(negative_nan).is_nan());
    assert!(corvid_float::abs(negative_nan).is_sign_positive());
    assert_eq!(corvid_float::abs(f32::NEG_INFINITY), f32::INFINITY);
}

#[test]
fn the_composed_rounding_matches_the_intrinsics() {
    for step in -400i32..400 {
        let x = step as f32 / 8.0;
        same(corvid_float::floor(x), x.floor(), "floor");
        same(corvid_float::ceil(x), x.ceil(), "ceil");
        same(corvid_float::trunc(x), x.trunc(), "trunc");
        same(corvid_float::round(x), x.round(), "round");
        same(corvid_float::abs(x), x.abs(), "abs");
    }
}

/// `ceil` is `-floor(-x)`, and the identity is exact because a negation is a
/// sign bit. The one value where a sloppier implementation would disagree with
/// the intrinsic is the negative zero it produces from an input in `(-1, 0)`.
#[test]
fn the_composed_ceiling_keeps_the_intrinsic_s_negative_zero() {
    let ours = corvid_float::ceil(-0.5);
    assert_eq!(ours, (-0.5f32).ceil());
    assert!(ours.is_sign_negative(), "ceil(-0.5) should be -0.0");
}

/// The other half-step trap: a `round` written as `trunc(x + 0.5)` answers one
/// for the value just below a half, because the addition rounds up before the
/// truncation ever runs. `const_soft_float` subtracts a quarter of an epsilon
/// from the half to avoid it, and this is the value that says so.
#[test]
fn rounding_the_value_just_below_a_half_gives_zero() {
    let just_under = f32::from_bits(0.5_f32.to_bits() - 1);
    assert_eq!(corvid_float::round(just_under), 0.0);
    assert_eq!(corvid_float::round(just_under), just_under.round());

    let wide_just_under = 0.499_999_999_999_999_94_f64;
    assert_eq!(corvid_float::wide::round(wide_just_under), 0.0);
    assert_eq!(
        corvid_float::wide::round(wide_just_under),
        wide_just_under.round()
    );
    // The spelling `corvid_fixed`'s `const` conversions use instead, because
    // they do not depend on this crate: add a half, let the cast truncate. It
    // answers one, which is the difference `wide::round`'s doc comment records.
    assert_eq!((wide_just_under + 0.5) as i64, 1);
}

/// Unlike [`f32::clamp`], which panics when its bounds cross. The workspace
/// forbids a panic in a library, so the upper bound simply wins -- and `NaN`
/// falls through to the low bound, which is what turns a gain that has gone
/// wrong into silence rather than into full volume.
#[test]
fn clamping_does_not_panic_on_crossed_bounds_or_nan() {
    // The ordinary case, from both directions.
    assert_eq!(corvid_float::clamp(5.0, 0.0, 1.0), 1.0);
    assert_eq!(corvid_float::clamp(-5.0, 0.0, 1.0), 0.0);
    assert_eq!(corvid_float::clamp(0.5, 0.0, 1.0), 0.5);

    // Crossed: `high` is tested first, so anything above it comes back as it.
    assert_eq!(corvid_float::clamp(0.5, 1.0, 0.0), 0.0);
    assert_eq!(corvid_float::clamp(-1.0, 1.0, 0.0), 1.0);

    assert_eq!(corvid_float::clamp(f32::NAN, 2.0, 3.0), 2.0);
    assert_eq!(corvid_float::wide::clamp(f64::NAN, 2.0, 3.0), 2.0);

    // The infinities are ordinary values to this one: above `high` and below
    // `low` respectively. `clamp_finite` is the one that reads them as faults.
    assert_eq!(corvid_float::clamp(f32::INFINITY, 0.0, 1.0), 1.0);
    assert_eq!(corvid_float::clamp(f32::NEG_INFINITY, 0.0, 1.0), 0.0);
}

/// The mixer's clamp. Everything non-finite comes back as `low`, which is
/// silence, and everything finite is clamped the ordinary way.
#[test]
fn clamping_finitely_sends_every_non_finite_to_the_low_bound() {
    assert_eq!(corvid_float::clamp_finite(f32::INFINITY, 0.0, 1.0), 0.0);
    assert_eq!(corvid_float::clamp_finite(f32::NEG_INFINITY, 0.0, 1.0), 0.0);
    assert_eq!(corvid_float::clamp_finite(f32::NAN, 0.0, 1.0), 0.0);

    assert_eq!(corvid_float::clamp_finite(0.5, 0.0, 1.0), 0.5);
    assert_eq!(corvid_float::clamp_finite(-5.0, 0.0, 1.0), 0.0);
    assert_eq!(corvid_float::clamp_finite(5.0, 0.0, 1.0), 1.0);
    // Finite, however large: `f32::MAX` is a value and not a fault.
    assert_eq!(corvid_float::clamp_finite(f32::MAX, 0.0, 1.0), 1.0);

    // The low bound is returned as written, so a mixer asking for silence in a
    // range that does not contain zero gets the bottom of its range.
    assert_eq!(corvid_float::clamp_finite(f32::NAN, -1.0, 1.0), -1.0);
}

/// The two clamps differ in two places and the doc comments name both: what an
/// infinity does, and -- because they test their bounds in opposite orders --
/// what crossed bounds do.
#[test]
fn the_two_clamps_part_company_on_infinities_and_on_crossed_bounds() {
    assert_eq!(corvid_float::clamp(f32::INFINITY, 0.0, 1.0), 1.0);
    assert_eq!(corvid_float::clamp_finite(f32::INFINITY, 0.0, 1.0), 0.0);

    // `clamp` tests `high` first, `clamp_finite` tests `low` first, so a value
    // between crossed bounds falls out of opposite arms.
    assert_eq!(corvid_float::clamp(0.5, 1.0, 0.0), 0.0);
    assert_eq!(corvid_float::clamp_finite(0.5, 1.0, 0.0), 1.0);

    // Where they still agree: below both, and above both.
    assert_eq!(corvid_float::clamp(-1.0, 1.0, 0.0), 1.0);
    assert_eq!(corvid_float::clamp_finite(-1.0, 1.0, 0.0), 1.0);
    assert_eq!(corvid_float::clamp(2.0, 1.0, 0.0), 0.0);
    assert_eq!(corvid_float::clamp_finite(2.0, 1.0, 0.0), 0.0);
}

/// The narrowing, and what it does with a value that will not fit.
#[test]
fn demoting_narrows_and_saturates_to_an_infinity() {
    assert_eq!(corvid_float::demote(1.0 / 3.0), 1.0_f32 / 3.0);
    assert_eq!(corvid_float::demote(f64::from(f32::MAX)), f32::MAX);
    assert_eq!(corvid_float::demote(1e300), f32::INFINITY);
    assert_eq!(corvid_float::demote(-1e300), f32::NEG_INFINITY);
    assert_eq!(corvid_float::demote(1e-300), 0.0);
    assert!(corvid_float::demote(f64::NAN).is_nan());
    same(corvid_float::demote(-0.0), -0.0, "demote(-0.0)");

    // Which is the pairing the doc comment claims: an overflow on the way down
    // arrives as an infinity, and `clamp_finite` is what turns it back into a
    // number a device can take.
    assert_eq!(
        corvid_float::clamp_finite(corvid_float::demote(1e300), 0.0, 1.0),
        0.0
    );
}

/// The wide half, spot-checked the same way. It is the same implementation a
/// word wider, so the point here is that the module exists and is wired up
/// rather than that the algorithm is different.
#[test]
fn the_wide_half_matches_its_intrinsics_too() {
    for step in -200i32..200 {
        let x = f64::from(step) / 16.0;
        assert!((corvid_float::wide::sin(x) - x.sin()).abs() < 1e-12);
        assert!((corvid_float::wide::sqrt(x.abs()) - x.abs().sqrt()).abs() < 1e-12);
        same_wide(corvid_float::wide::round(x), x.round(), "wide round");
        same_wide(corvid_float::wide::floor(x), x.floor(), "wide floor");
        same_wide(corvid_float::wide::ceil(x), x.ceil(), "wide ceil");
        same_wide(corvid_float::wide::trunc(x), x.trunc(), "wide trunc");
        same_wide(corvid_float::wide::abs(x), x.abs(), "wide abs");
    }
    for x in [
        0.0_f64,
        -0.0,
        -0.5,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        f64::MIN_POSITIVE,
        f64::MAX,
        f64::MIN,
    ] {
        same_wide(corvid_float::wide::floor(x), x.floor(), "wide floor");
        same_wide(corvid_float::wide::ceil(x), x.ceil(), "wide ceil");
        same_wide(corvid_float::wide::round(x), x.round(), "wide round");
        same_wide(corvid_float::wide::trunc(x), x.trunc(), "wide trunc");
        same_wide(corvid_float::wide::abs(x), x.abs(), "wide abs");
        same_wide(corvid_float::wide::sqrt(x), x.sqrt(), "wide sqrt");
    }
    same_wide(
        corvid_float::wide::sqrt(2.0),
        2.0_f64.sqrt(),
        "wide sqrt(2)",
    );
    same_wide(
        corvid_float::wide::copysign(3.0, -1.0),
        3.0_f64.copysign(-1.0),
        "wide copysign",
    );
}
