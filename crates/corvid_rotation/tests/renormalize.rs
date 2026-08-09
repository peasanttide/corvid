//! `renormalize_fast`: that it names the same rotation as `renormalize`, and
//! what it leaves behind that the exact one does not.
//!
//! The fast form settles at the edge of a deadband rather than on the sphere,
//! so what is checked is that the drift stays inside that deadband however
//! many compositions run through it, and that the versor it leaves is one
//! `from_xyzw` may well reject.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::suboptimal_flops,
    clippy::items_after_statements,
    reason = "tests reach into raw bit patterns on purpose, and their f64 references are written as plain arithmetic so they stay independent of the implementation"
)]

mod common;

use common::Rng;
use corvid_fixed::{Angle32, Signed32};
use corvid_rotation::Versor;
use corvid_vector::Direction;

const X: Direction = Direction::new(Signed32::MAX, Signed32::ZERO, Signed32::ZERO);
const Y: Direction = Direction::new(Signed32::ZERO, Signed32::MAX, Signed32::ZERO);
const Z: Direction = Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MAX);

#[test]
fn renormalize_fast_names_the_same_rotation_as_renormalize() {
    let mut rng = Rng::new(0xF457_0002);
    let mut worst = 0.0f64;
    for _ in 0..20_000 {
        let versor = common::random_versor(&mut rng);
        let exact = common::to_f64_quaternion(versor.renormalize());
        let fast = common::to_f64_quaternion(versor.renormalize_fast());
        worst = worst.max(common::angle_degrees(exact, fast));
    }
    // Four decimal digits of a quaternion is well under a thousandth of a
    // degree -- below what any renderer or physics step resolves.
    assert!(worst < 1e-3, "worst disagreement {worst} degrees");
}

#[test]
fn renormalize_fast_is_exact_on_the_axis_aligned_rotations() {
    // The reduction's `0.25` case is taken by hand before either `rsqrt`, so
    // the identity and the quarter turns cost the approximation nothing.
    assert_eq!(Versor::IDENTITY.renormalize_fast(), Versor::IDENTITY);
    for versor in [
        Versor::from_axis_angle(X, Angle32::from_degrees(90.0)),
        Versor::from_axis_angle(Y, Angle32::from_degrees(180.0)),
        Versor::from_axis_angle(Z, Angle32::from_degrees(270.0)),
    ] {
        assert_eq!(versor.renormalize_fast(), versor.renormalize());
    }
}

#[test]
fn renormalize_fast_bounds_composition_drift_with_a_deadband() {
    // The documented characteristic, and the one that is easy to get wrong: the
    // approximate tier does *not* diverge under repeated use -- it bounds the
    // drift -- but it bounds it at the edge of its own `2^-15` deadband rather
    // than at a last bit, because drift finer than that is invisible to it.
    let axis = Direction::new(
        Signed32::from_f64(0.3),
        Signed32::from_f64(0.5),
        Signed32::from_f64(0.81),
    )
    .normalize()
    .expect("a nonzero axis has a direction");
    let step = Versor::from_axis_angle(axis, Angle32::from_degrees(37.0));

    let norm_error = |q: Versor| {
        let [x, y, z, w] = common::to_f64_quaternion(q);
        (x * x + y * y + z * z + w * w - 1.0).abs()
    };

    let mut bare = Versor::IDENTITY;
    let mut fast = Versor::IDENTITY;
    let mut exact = Versor::IDENTITY;
    let (mut worst_fast, mut worst_exact) = (0.0f64, 0.0f64);
    // Long enough for the unrenormalized random walk to pull clear of the
    // deadband; at 20,000 the two are still the same size.
    for _ in 0..200_000 {
        bare = bare.compose(step);
        fast = fast.compose(step).renormalize_fast();
        exact = exact.compose(step).renormalize();
        worst_fast = worst_fast.max(norm_error(fast));
        worst_exact = worst_exact.max(norm_error(exact));
    }

    // It bounds the drift: an order of magnitude better than leaving it alone.
    assert!(
        worst_fast < norm_error(bare) / 10.0,
        "fast held {worst_fast} against an unrenormalized {}",
        norm_error(bare)
    );
    // At its deadband, which is `2^-16` in the squared norm.
    assert!(
        (1e-6..3e-5).contains(&worst_fast),
        "fast settled at {worst_fast}, not at its deadband"
    );
    // And three orders of magnitude looser than the exact tier, which is the
    // whole of what choosing it costs.
    assert!(
        worst_exact < 1e-8 && worst_fast > worst_exact * 1_000.0,
        "exact held {worst_exact} against fast's {worst_fast}"
    );
}

#[test]
fn renormalize_fast_can_leave_a_versor_that_from_xyzw_rejects() {
    // The consequence of that deadband, pinned because it is a real trap: the
    // level `renormalize_fast` settles at sits right on `from_xyzw`'s unit
    // tolerance, so a long-composed versor eventually stops round-tripping.
    let axis = Direction::new(
        Signed32::from_f64(0.3),
        Signed32::from_f64(0.5),
        Signed32::from_f64(0.81),
    )
    .normalize()
    .expect("a nonzero axis has a direction");
    let step = Versor::from_axis_angle(axis, Angle32::from_degrees(37.0));

    let round_trips = |q: Versor| {
        let [x, y, z, w] = q.to_xyzw();
        Versor::from_xyzw(x, y, z, w).is_some()
    };

    let mut fast = Versor::IDENTITY;
    let mut exact = Versor::IDENTITY;
    let mut fast_rejected = 0u32;
    for _ in 0..20_000 {
        fast = fast.compose(step).renormalize_fast();
        exact = exact.compose(step).renormalize();
        if !round_trips(fast) {
            fast_rejected += 1;
        }
        // The exact tier never does this, which is what makes it the default.
        assert!(round_trips(exact), "renormalize left a non-unit versor");
    }
    assert!(
        fast_rejected > 0,
        "renormalize_fast round-tripped every time, so its documented trap is gone"
    );
}
