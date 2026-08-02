//! The rotation family against an `f64` reference, and the axis conventions.
//!
//! These are the tests that pin the coordinate convention down: **+X right,
//! +Y forward, +Z up**, yaw about +Z, pitch about +X, roll about +Y, ZXY
//! intrinsic, `right = forward × up`.

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
use corvid_fixed::{Angle32, I2F30, Pitch32, Signed32};
use corvid_rotation::{Basis, Versor};
use corvid_vector::Direction;

const X: Direction = Direction::new(Signed32::MAX, Signed32::ZERO, Signed32::ZERO);
const Y: Direction = Direction::new(Signed32::ZERO, Signed32::MAX, Signed32::ZERO);
const Z: Direction = Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MAX);

/// Component tolerance for a direction that has been through a rotation.
const AXIS_TOLERANCE: f64 = 1e-4;

#[test]
fn yaw_turns_about_z_pitch_about_x_roll_about_y() {
    // A quarter turn of yaw takes forward (+Y) onto left (-X).
    let yaw = Basis::from_yaw_pitch_roll(Angle32::from_degrees(90.0), Pitch32::ZERO, Angle32::ZERO);
    assert!(
        common::direction_within(yaw.forward(), common::neg(X), AXIS_TOLERANCE),
        "yaw forward is {:?}",
        yaw.forward()
    );
    assert!(common::direction_within(yaw.up(), Z, AXIS_TOLERANCE));
    assert!(common::direction_within(yaw.right(), Y, AXIS_TOLERANCE));

    // A quarter turn of pitch takes forward (+Y) onto up (+Z), leaving right
    // alone, because pitch is about +X.
    let pitch =
        Basis::from_yaw_pitch_roll(Angle32::ZERO, Pitch32::from_degrees(90.0), Angle32::ZERO);
    assert!(
        common::direction_within(pitch.forward(), Z, AXIS_TOLERANCE),
        "pitch forward is {:?}",
        pitch.forward()
    );
    assert!(common::direction_within(pitch.right(), X, AXIS_TOLERANCE));

    // A quarter turn of roll takes right (+X) onto down (-Z), leaving forward
    // alone, because roll is about +Y.
    let roll =
        Basis::from_yaw_pitch_roll(Angle32::ZERO, Pitch32::ZERO, Angle32::from_degrees(90.0));
    assert!(
        common::direction_within(roll.forward(), Y, AXIS_TOLERANCE),
        "roll forward is {:?}",
        roll.forward()
    );
    assert!(
        common::direction_within(roll.right(), common::neg(Z), AXIS_TOLERANCE),
        "roll right is {:?}",
        roll.right()
    );
}

#[test]
fn all_three_angles_zero_is_the_identity() {
    assert_eq!(
        Basis::from_yaw_pitch_roll(Angle32::ZERO, Pitch32::ZERO, Angle32::ZERO),
        Basis::IDENTITY
    );
}

#[test]
fn euler_composition_is_zxy_intrinsic() {
    // R = Rz(yaw) . Rx(pitch) . Ry(roll), spelled out as three composes.
    let mut rng = Rng::new(0x2A40_0001);
    for _ in 0..5_000 {
        let yaw = Angle32::from_bits(rng.next_u32());
        let pitch = Pitch32::from_degrees(rng.next_unit() * 89.0);
        let roll = Angle32::from_bits(rng.next_u32());

        let combined = Basis::from_yaw_pitch_roll(yaw, pitch, roll);
        let separate = Basis::from_yaw_pitch_roll(yaw, Pitch32::ZERO, Angle32::ZERO)
            .compose(Basis::from_yaw_pitch_roll(
                Angle32::ZERO,
                pitch,
                Angle32::ZERO,
            ))
            .compose(Basis::from_yaw_pitch_roll(
                Angle32::ZERO,
                Pitch32::ZERO,
                roll,
            ));

        assert!(
            combined.abs_diff_eq(separate, I2F30::from_bits(1 << 14)),
            "ZXY composition disagrees:\n  {combined:?}\n  {separate:?}"
        );
    }
}

#[test]
fn yaw_pitch_roll_round_trips() {
    let mut rng = Rng::new(0x0B50_0001);
    for _ in 0..20_000 {
        let yaw = Angle32::from_bits(rng.next_u32());
        // Stay off the gimbal-lock poles, where yaw and roll are degenerate.
        let pitch = Pitch32::from_degrees(rng.next_unit() * 85.0);
        let roll = Angle32::from_bits(rng.next_u32());

        let m = Basis::from_yaw_pitch_roll(yaw, pitch, roll);
        let (y2, p2, r2) = m.to_yaw_pitch_roll();
        let back = Basis::from_yaw_pitch_roll(y2, p2, r2);
        assert!(
            m.abs_diff_eq(back, I2F30::from_bits(1 << 16)),
            "round trip lost the rotation:\n  {m:?}\n  {back:?}"
        );
    }
}

/// The band just short of the pole, which the round-trip test above skips.
///
/// The degenerate branch throws roll away, so it must not fire while roll is
/// still determined. It used to fire from 89.84°, where `cos(pitch)` is
/// `2.8e-3` and the discarded roll cost 0.30° of round-trip error — 60× the
/// 0.005° the codec itself carries — right where a head-tracked pose looking
/// nearly straight up lives.
#[test]
fn near_the_poles_roll_is_still_recovered() {
    for &pitch_degrees in &[89.0, 89.5, 89.83, 89.85, 89.9, 89.95] {
        for yi in 0..17 {
            for ri in 0..17 {
                let yaw = Angle32::from_degrees(-180.0 + 360.0 * f64::from(yi) / 17.0);
                let roll = Angle32::from_degrees(-180.0 + 360.0 * f64::from(ri) / 17.0);
                let pitch = Pitch32::from_degrees(pitch_degrees);

                let m = Basis::from_yaw_pitch_roll(yaw, pitch, roll);
                let (y2, p2, r2) = m.to_yaw_pitch_roll();
                let back = Basis::from_yaw_pitch_roll(y2, p2, r2);
                let error = m.angle_to(back).to_degrees();
                assert!(
                    error < 0.01,
                    "pitch {pitch_degrees}, yaw {yaw:?}, roll {roll:?}: lost {error}°"
                );
            }
        }
    }
}

#[test]
fn at_the_poles_roll_is_folded_into_yaw() {
    // Pitch at a quarter turn leaves only yaw + roll determined; the whole turn
    // is attributed to yaw and roll comes back zero. The rotation still round-
    // trips, which is what actually matters.
    let straight_up = Basis::from_yaw_pitch_roll(
        Angle32::from_degrees(30.0),
        Pitch32::MAX,
        Angle32::from_degrees(40.0),
    );
    let (_, pitch, roll) = straight_up.to_yaw_pitch_roll();
    assert_eq!(roll, Angle32::ZERO);
    assert!((pitch.to_degrees() - 90.0).abs() < 0.01);

    let (y, p, r) = straight_up.to_yaw_pitch_roll();
    assert!(straight_up.abs_diff_eq(
        Basis::from_yaw_pitch_roll(y, p, r),
        I2F30::from_bits(1 << 16)
    ));

    // The other pole, where the free parameter is yaw - roll rather than
    // yaw + roll and the recovered sine comes back negated.
    let straight_down = Basis::from_yaw_pitch_roll(
        Angle32::from_degrees(30.0),
        Pitch32::MIN,
        Angle32::from_degrees(40.0),
    );
    let (y, p, r) = straight_down.to_yaw_pitch_roll();
    assert_eq!(r, Angle32::ZERO);
    assert!((p.to_degrees() + 90.0).abs() < 0.01);
    assert!(straight_down.abs_diff_eq(
        Basis::from_yaw_pitch_roll(y, p, r),
        I2F30::from_bits(1 << 16)
    ));
}

#[test]
fn look_to_produces_the_documented_axes() {
    // Looking along +Y with +Z up is the identity.
    let m = Basis::look_to(Y, Z).expect("perpendicular");
    assert!(
        m.abs_diff_eq(Basis::IDENTITY, I2F30::from_bits(1 << 8)),
        "{m:?}"
    );

    // right = forward x up, consistent with X x Y = Z. Looking along +X with
    // +Z up puts right at -Y.
    let m = Basis::look_to(X, Z).expect("perpendicular");
    assert!(common::direction_within(m.forward(), X, AXIS_TOLERANCE));
    assert!(
        common::direction_within(m.right(), common::neg(Y), AXIS_TOLERANCE),
        "right is {:?}",
        m.right()
    );
    assert!(common::direction_within(m.up(), Z, AXIS_TOLERANCE));
}

#[test]
fn look_to_orthonormalizes_an_up_that_was_not_perpendicular() {
    // A tilted up vector still gives a genuine rotation, with forward exact.
    let tilted = Direction::new(
        Signed32::ZERO,
        Signed32::from_f64(0.6),
        Signed32::from_f64(0.8),
    );
    let m = Basis::look_to(Y, tilted).expect("not parallel");
    assert!(common::direction_within(m.forward(), Y, AXIS_TOLERANCE));
    assert!(
        Basis::from_rows(m.to_rows()).is_some(),
        "look_to produced a non-rotation"
    );
}

/// `up` longer than one, which is what an engine boundary hands over.
///
/// `Direction` permits a length of up to `√3`, and `cross` rounds each term
/// onto `Signed32`'s `±1` — so a cross product longer than one comes back
/// clamped per axis, which changes its *direction*. The tilted case above uses
/// a unit `up` and does not reach it.
#[test]
fn look_to_orthonormalizes_an_up_that_was_not_unit_length() {
    let mut rng = Rng::new(0x100_0009);
    for _ in 0..20_000 {
        let forward = Direction::new(
            Signed32::from_f64(rng.next_unit()),
            Signed32::from_f64(rng.next_unit()),
            Signed32::from_f64(rng.next_unit()),
        );
        // Deliberately not normalized: lengths run past 1 up to √3.
        let up = Direction::new(
            Signed32::from_f64(rng.next_unit()),
            Signed32::from_f64(rng.next_unit()),
            Signed32::from_f64(rng.next_unit()),
        );
        let Some(forward) = forward.normalize() else {
            continue;
        };
        if let Some(m) = Basis::look_to(forward, up) {
            assert!(
                Basis::from_rows(m.to_rows()).is_some(),
                "look_to produced a matrix its own validator rejects: {m:?}"
            );
        }
    }
}

#[test]
fn look_to_returns_none_on_parallel_or_zero_input() {
    assert_eq!(Basis::look_to(Z, Z), None);
    assert_eq!(Basis::look_to(Z, common::neg(Z)), None);
    // A zero vector has no direction to normalize, and the parallel test alone
    // would not catch it.
    assert_eq!(Basis::look_to(Direction::ZERO, Z), None);
    assert_eq!(Basis::look_to(Y, Direction::ZERO), None);
    assert_eq!(Basis::look_to(Direction::ZERO, Direction::ZERO), None);
    // The versor form agrees.
    assert_eq!(Versor::look_to(Z, Z), None);
    assert!(Versor::look_to(Y, Z).is_some());
}

#[test]
fn from_rotation_arc_maps_from_onto_to() {
    let mut rng = Rng::new(0x0A2C_0001);
    for _ in 0..20_000 {
        let from = common::random_direction(&mut rng);
        let to = common::random_direction(&mut rng);
        let m = Basis::from_rotation_arc(from, to);
        assert!(
            common::direction_within(m.rotate_direction(from), to, 1e-3),
            "{from:?} -> {:?}, wanted {to:?}",
            m.rotate_direction(from)
        );
    }
}

#[test]
fn from_rotation_arc_is_total_at_the_degenerate_cases() {
    // Identical inputs give the identity.
    let same = Basis::from_rotation_arc(Y, Y);
    assert!(
        same.abs_diff_eq(Basis::IDENTITY, I2F30::from_bits(1 << 8)),
        "{same:?}"
    );

    // Antipodal inputs give some half turn about a perpendicular axis — the
    // honest answer when the shortest arc is not unique.
    for axis in [X, Y, Z] {
        let flip = Basis::from_rotation_arc(axis, common::neg(axis));
        assert!(
            common::direction_within(flip.rotate_direction(axis), common::neg(axis), 1e-3),
            "half turn about {axis:?} gave {:?}",
            flip.rotate_direction(axis)
        );
        assert!(Basis::from_rows(flip.to_rows()).is_some());
    }
}

#[test]
fn axis_angle_round_trips() {
    let mut rng = Rng::new(0xA11E_0001);
    for _ in 0..20_000 {
        let axis = common::random_direction(&mut rng);
        let angle = Angle32::from_degrees(rng.next_unit().abs() * 178.0 + 1.0);
        let q = Versor::from_axis_angle(axis, angle);
        let (recovered_axis, recovered_angle) = q.to_axis_angle();

        // Rebuilding from what came back must give the same rotation, which is
        // the claim that matters — the axis may come back negated with the
        // angle measured the other way.
        let rebuilt = Versor::from_axis_angle(recovered_axis, recovered_angle);
        assert!(
            q.to_basis()
                .abs_diff_eq(rebuilt.to_basis(), I2F30::from_bits(1 << 16)),
            "axis-angle round trip lost the rotation: {axis:?} at {} degrees",
            angle.to_degrees()
        );
    }
}

#[test]
fn from_axis_angle_turns_the_right_way() {
    // A quarter turn about +Z takes +Y onto -X, matching yaw.
    let q = Versor::from_axis_angle(Z, Angle32::from_degrees(90.0));
    assert!(
        common::direction_within(q.rotate_direction(Y), common::neg(X), AXIS_TOLERANCE),
        "+Y rotated to {:?}",
        q.rotate_direction(Y)
    );
    // A full turn is the identity.
    let full = Versor::from_axis_angle(Z, Angle32::ZERO);
    assert_eq!(full.to_basis(), Basis::IDENTITY);
}

#[test]
fn to_axis_angle_reports_the_identity_as_a_zero_turn() {
    let (_, angle) = Versor::IDENTITY.to_axis_angle();
    assert_eq!(angle, Angle32::ZERO);
}

#[test]
fn rotate_towards_never_overshoots() {
    let mut rng = Rng::new(0x207A_0001);
    let step = Angle32::from_degrees(5.0);
    for _ in 0..10_000 {
        let a = common::random_versor(&mut rng);
        let b = common::random_versor(&mut rng);
        let remaining_before = a.angle_to(b).to_degrees();
        let moved = a.rotate_towards(b, step);

        let travelled = a.angle_to(moved).to_degrees();
        assert!(travelled <= 5.0 + 0.05, "travelled {travelled} degrees");
        assert!(travelled <= remaining_before + 0.05);
        // And it makes progress rather than wandering.
        assert!(
            moved.angle_to(b).to_degrees() <= remaining_before + 0.05,
            "moved away from the target"
        );
    }
}

#[test]
fn rotate_towards_lands_exactly_when_the_step_covers_the_gap() {
    let a = Versor::IDENTITY;
    let b = Versor::from_axis_angle(Z, Angle32::from_degrees(3.0));
    assert_eq!(a.rotate_towards(b, Angle32::from_degrees(90.0)), b);
    assert_eq!(a.rotate_towards(a, Angle32::from_degrees(1.0)), a);
}

#[test]
fn the_operation_family_is_available_in_const_context() {
    const AXIS: Direction = Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MAX);
    const QUARTER: Versor = Versor::from_axis_angle(AXIS, Angle32::QUARTER_TURN);
    const POSE: Basis = Basis::from_yaw_pitch_roll(Angle32::ZERO, Pitch32::ZERO, Angle32::ZERO);
    const AIMED: Option<Basis> = Basis::look_to(
        Direction::new(Signed32::ZERO, Signed32::MAX, Signed32::ZERO),
        AXIS,
    );
    const ARC: Versor = Versor::from_rotation_arc(AXIS, AXIS);

    assert_eq!(POSE, Basis::IDENTITY);
    assert_eq!(AIMED, Some(Basis::IDENTITY));
    assert!(QUARTER.to_basis().abs_diff_eq(
        Basis::from_yaw_pitch_roll(Angle32::QUARTER_TURN, Pitch32::ZERO, Angle32::ZERO),
        I2F30::from_bits(1 << 12)
    ));
    assert!(
        ARC.to_basis()
            .abs_diff_eq(Basis::IDENTITY, I2F30::from_bits(1 << 8))
    );
}

// --- renormalize_fast ------------------------------------------------------

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
    // degree — below what any renderer or physics step resolves.
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
    // approximate tier does *not* diverge under repeated use — it bounds the
    // drift — but it bounds it at the edge of its own `2^-15` deadband rather
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

/// A unit direction from three `f64` components.
fn unit(x: f64, y: f64, z: f64) -> Direction {
    let norm = (x * x + y * y + z * z).sqrt();
    Direction::new(
        Signed32::from_f64(x / norm),
        Signed32::from_f64(y / norm),
        Signed32::from_f64(z / norm),
    )
}

/// The `f64` dot product of two directions.
fn dot(a: Direction, b: Direction) -> f64 {
    a.x().to_f64() * b.x().to_f64()
        + a.y().to_f64() * b.y().to_f64()
        + a.z().to_f64() * b.z().to_f64()
}

#[test]
fn from_rotation_arc_survives_near_antipodal_input() {
    // The degenerate case has to be recognized from the *dot product* alone.
    // Testing the cross product for exact zero as well narrows the branch to
    // exactly opposite inputs, and everything between there and opposite falls
    // through to a formula whose two terms have both underflowed to noise —
    // which came back a rotation missing `to` by up to a hundred degrees.
    let mut rng = Rng::new(0x4152_4300_0000_0001);
    for _ in 0..50_000 {
        let from = unit(rng.next_unit(), rng.next_unit(), rng.next_unit());
        // Opposite, nudged by an amount that sweeps every decade down to the
        // last bit of `Signed32`.
        let nudge = 10f64.powf(-(rng.next_unit().abs() * 12.0));
        let to = unit(
            (-from.x().to_f64()).mul_add(1.0, nudge * rng.next_unit()),
            (-from.y().to_f64()).mul_add(1.0, nudge * rng.next_unit()),
            (-from.z().to_f64()).mul_add(1.0, nudge * rng.next_unit()),
        );

        let q = Versor::from_rotation_arc(from, to);
        let [x, y, z, w] = q.to_xyzw();
        assert!(
            Versor::from_xyzw(x, y, z, w).is_some(),
            "near-antipodal arc left the unit sphere: {q:?}"
        );
        // A half turn about *some* perpendicular axis is the honest answer
        // here, and it lands within the window's own half-width of `to`.
        let landed = dot(q.rotate_direction(from), to);
        assert!(
            landed > 0.999_999,
            "from_rotation_arc({from:?}, {to:?}) landed at {landed}"
        );
    }
}

#[test]
fn look_to_stays_orthonormal_when_forward_and_up_nearly_coincide() {
    // `Direction::cross` divides its `i64` terms back onto the unit scale, so
    // for nearly parallel operands almost nothing survives and the normalize
    // that follows amplifies the rounding. `look_to` must not build its frame
    // that way: it reported `Some` for a matrix skewed by as much as 30°, which
    // `from_rows` rejects and which is not a rotation at all.
    let mut rng = Rng::new(0x4C4F_4F4B_0000_0001);
    let mut built = 0u32;
    for _ in 0..50_000 {
        let forward = unit(rng.next_unit(), rng.next_unit(), rng.next_unit());
        let nudge = 10f64.powf(-(rng.next_unit().abs() * 12.0));
        let up = unit(
            forward.x().to_f64().mul_add(1.0, nudge * rng.next_unit()),
            forward.y().to_f64().mul_add(1.0, nudge * rng.next_unit()),
            forward.z().to_f64().mul_add(1.0, nudge * rng.next_unit()),
        );

        if let Some(basis) = Basis::look_to(forward, up) {
            built += 1;
            assert!(
                Basis::from_rows(basis.to_rows()).is_some(),
                "look_to({forward:?}, {up:?}) built a frame that is not a rotation: {basis:?}"
            );
            assert!(
                dot(basis.right(), basis.forward()).abs() < 1e-6,
                "look_to left right and forward non-perpendicular: {basis:?}"
            );
        }
    }
    assert!(built > 1_000, "the degenerate band swallowed every sample");
}
