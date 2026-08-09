//! `look_to`, the arcs, axis-angle and the step, against an `f64` reference.
//!
//! What each of these owes is an axis convention and a bound, and both are
//! checked against a reference worked out in `f64` rather than against another
//! call into the crate. The Euler family is in `tests/euler.rs` and the fast
//! renormalization in `tests/renormalize.rs`.

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
/// `Direction` permits a length of up to `sqrt(3)`, and `cross` rounds each term
/// onto `Signed32`'s `+/-1` -- so a cross product longer than one comes back
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
        // Deliberately not normalized: lengths run past 1 up to sqrt(3).
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

    // Antipodal inputs give some half turn about a perpendicular axis -- the
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
        // the claim that matters -- the axis may come back negated with the
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
    // through to a formula whose two terms have both underflowed to noise --
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
    // that way: it reported `Some` for a matrix skewed by as much as 30 deg, which
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
