//! The transcendentals, held to a count of last bits.
//!
//! Neither this crate's software sine nor the platform's libm is correctly
//! rounded, so there is no bit pattern to hold either to and pretending
//! otherwise would produce a test that passes on this machine only. What is
//! asserted instead is how many representable values apart the two answers may
//! be, which is a claim the README makes and this is where it rests.

#![allow(
    clippy::float_cmp,
    reason = "the poles and the reciprocal of zero are exact cases: what they produce is an infinity or a specific bit pattern, and a tolerance would not see a change to either"
)]
#![allow(
    clippy::cast_precision_loss,
    reason = "the loop counters are small integers being turned into the sample points, which is the standard way to sweep a range and is exact for every value they take"
)]
#![allow(
    clippy::suboptimal_flops,
    reason = "the tolerance expressions are written as `scale * relative + absolute` because that is how a tolerance reads; folding them into a `mul_add` would be faster and less legible in code whose job is to be read"
)]

mod common;

use common::ulps;

/// Over a couple of turns either side of zero, which is the range a frustum's
/// half-angle and a gain curve actually live in. `const_soft_float`'s argument
/// reduction is `rem_pio2`, so this is also what checks that the reduction is
/// there at all.
///
/// A last bit, not the `1e-6` an earlier version of this test allowed: the
/// README claims these agree with the intrinsic to within one representable
/// value and this is the assertion that claim rests on. Eight last bits of slack
/// would let a real regression through unnoticed.
#[test]
fn sines_and_cosines_match_the_intrinsic_to_a_last_bit() {
    for step in -800i32..800 {
        let x = step as f32 / 64.0;
        let (ours, theirs) = (corvid_float::sin(x), x.sin());
        assert!(ulps(ours, theirs) <= 1, "sin({x}): {ours} vs {theirs}");
        let (ours, theirs) = (corvid_float::cos(x), x.cos());
        assert!(ulps(ours, theirs) <= 1, "cos({x}): {ours} vs {theirs}");
    }
}

/// Well past where the reduction has any easy job left. A `sin` that dropped
/// `rem_pio2` would still pass the sweep above and would be nonsense here.
#[test]
fn sines_and_cosines_survive_an_argument_no_reduction_wants() {
    for x in [1.0e7_f32, 1.0e12, 1.0e20, f32::MAX] {
        assert!(ulps(corvid_float::sin(x), x.sin()) <= 1, "sin({x:e})");
        assert!(ulps(corvid_float::cos(x), x.cos()) <= 1, "cos({x:e})");
    }
    assert!(corvid_float::sin(f32::INFINITY).is_nan());
    assert!(corvid_float::cos(f32::INFINITY).is_nan());
    assert!(corvid_float::sin(f32::NAN).is_nan());
}

/// The composed one. Away from the poles, where both answers are enormous and
/// neither is useful.
///
/// Two last bits rather than one, because a quotient of two values that are each
/// a last bit out is two last bits out. That is arithmetic rather than slack.
#[test]
fn tangents_match_the_intrinsic_away_from_the_poles() {
    for step in -100i32..100 {
        let x = step as f32 / 128.0;
        let (ours, theirs) = (corvid_float::tan(x), x.tan());
        assert!(ulps(ours, theirs) <= 2, "tan({x}): {ours} vs {theirs}");
    }
}

/// The trap [`corvid_float::tan`]'s doc comment is about.
///
/// No `f32` is pi/2, so the cosine never actually reaches zero and the tangent
/// at the pole is not an infinity -- it is a large finite number whose sign
/// depends on which side of pi/2 the argument rounded to. `FRAC_PI_2` rounds
/// just past, so the answer there is negative, and a caller screening its
/// frustum for a non-finite focal length would pass a mirrored one straight
/// through. This test exists so that if the crate ever does start returning an
/// infinity there, the doc comment gets rewritten with it.
#[test]
fn a_tangent_at_the_pole_is_large_finite_and_carries_the_sign_of_the_rounding() {
    let over = corvid_float::consts::FRAC_PI_2;
    let under = f32::from_bits(over.to_bits() - 1);

    assert!(over > core::f32::consts::FRAC_PI_2 || under < over);
    for x in [over, under] {
        let ours = corvid_float::tan(x);
        assert!(ours.is_finite(), "tan({x}) = {ours} should be finite");
        assert!(ours.abs() > 1.0e6, "tan({x}) = {ours} should be enormous");
        assert!(ulps(ours, x.tan()) <= 2, "tan({x}): {ours} vs {}", x.tan());
    }
    assert!(
        corvid_float::tan(over) < 0.0,
        "pi/2 rounds up, so tan is below"
    );
    assert!(
        corvid_float::tan(under) > 0.0,
        "and the value below it is above"
    );
}

/// The reciprocal's documented edge, which is a real infinity because it is a
/// real division by a real zero -- unlike the tangent's.
#[test]
fn a_reciprocal_of_zero_is_an_infinity_rather_than_a_panic() {
    assert_eq!(corvid_float::recip(0.0), f32::INFINITY);
    assert_eq!(corvid_float::recip(-0.0), f32::NEG_INFINITY);
    assert_eq!(corvid_float::recip(f32::INFINITY), 0.0);
    assert!(corvid_float::recip(f32::NAN).is_nan());
    assert_eq!(corvid_float::recip(4.0), 0.25);
    assert_eq!(corvid_float::wide::recip(0.0), f64::INFINITY);
}

#[test]
fn hypotenuses_match_the_intrinsic_in_the_range_a_camera_works_in() {
    for x in -20i32..20 {
        for y in -20i32..20 {
            let (x, y) = (x as f32 / 4.0, y as f32 / 4.0);
            let (ours, theirs) = (corvid_float::hypot(x, y), x.hypot(y));
            assert!(
                (ours - theirs).abs() <= theirs * 1e-6 + 1e-7,
                "hypot({x}, {y}): {ours} vs {theirs}"
            );
        }
    }
}

/// And the two places outside that range where it does not, both of which the
/// doc comment names. Forming the squares before the root is the whole reason,
/// and a future implementation that scales its arguments first would fail this
/// test -- which is the point: the doc comment would have to change with it.
#[test]
fn hypotenuses_overflow_and_collapse_where_the_documentation_says_they_do() {
    // Above the top: the square overflows, the root of an infinity is one.
    assert!(corvid_float::hypot(1.9e19, 0.0).is_infinite());
    assert!(1.9e19_f32.hypot(0.0).is_finite());
    // And just inside it, where the square still fits.
    assert_eq!(corvid_float::hypot(1.8e19, 0.0), 1.8e19);

    // Below the bottom: the square is zero, and so is its root. This is the
    // quieter failure, which is why it is written down.
    assert_eq!(
        corvid_float::hypot(f32::MIN_POSITIVE, f32::MIN_POSITIVE),
        0.0
    );
    assert!(f32::MIN_POSITIVE.hypot(f32::MIN_POSITIVE) > 0.0);

    // The wide half has the same shape of failure, far enough out that no
    // caller reaches it.
    assert!(corvid_float::wide::hypot(1.4e154, 0.0).is_infinite());
    assert!(corvid_float::wide::hypot(1.3e154, 0.0).is_finite());
}
