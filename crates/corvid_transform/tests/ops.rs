//! The game-dev operation family, and every `None` case.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::many_single_char_names,
    reason = "x, y, z and p are the names this subject matter uses"
)]

mod common;

use common::Rng;
use corvid_transform::{
    Angle32, Direction, Factor32, FineTransform, GlobalPoint, I24F8, Rotation, Signed32, Transform,
};

const UP: Direction = Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MAX);

#[test]
fn look_at_produces_the_documented_axes() {
    let eye = GlobalPoint::ZERO;
    let target = GlobalPoint::new(I24F8::ZERO, I24F8::from_f64(10.0), I24F8::ZERO);

    // Looking along +Y with +Z up is the identity rotation.
    let t = Transform::look_at(eye, target, UP).expect("perpendicular");
    assert_eq!(t.position(), eye);
    assert!(
        t.rotation()
            .to_versor()
            .angle_to(Rotation::IDENTITY.to_versor())
            .to_degrees()
            < 0.2
    );

    // The target is dead ahead in eye space.
    let local = t.to_local(target).expect("in range");
    assert!(
        local.x().abs().to_f64() < 0.1,
        "x is {}",
        local.x().to_f64()
    );
    assert!(local.y().to_f64() > 9.8, "y is {}", local.y().to_f64());
    assert!(
        local.z().abs().to_f64() < 0.1,
        "z is {}",
        local.z().to_f64()
    );
}

#[test]
fn look_at_aims_at_arbitrary_targets() {
    let mut rng = Rng::new(0x0B50_0001);
    for _ in 0..10_000 {
        let eye = GlobalPoint::new(
            I24F8::from_f64(rng.next_unit() * 1000.0),
            I24F8::from_f64(rng.next_unit() * 1000.0),
            I24F8::from_f64(rng.next_unit() * 1000.0),
        );
        let target = GlobalPoint::new(
            I24F8::from_f64(rng.next_unit() * 1000.0),
            I24F8::from_f64(rng.next_unit() * 1000.0),
            I24F8::from_f64(rng.next_unit() * 1000.0),
        );
        let Some(t) = Transform::look_at(eye, target, UP) else {
            continue;
        };

        // The target lands on the forward axis in eye space.
        let Some(local) = t.to_local(target) else {
            continue;
        };
        let distance = eye.distance(target).to_f64();
        if distance < 1.0 {
            continue;
        }
        assert!(
            local.y().to_f64() > distance * 0.99,
            "target is not ahead: local {local:?}, distance {distance}"
        );
    }
}

#[test]
fn look_at_returns_none_only_on_the_degenerate_cases() {
    let eye = GlobalPoint::ZERO;

    // Straight up, parallel to `up`.
    let above = GlobalPoint::new(I24F8::ZERO, I24F8::ZERO, I24F8::from_f64(10.0));
    assert_eq!(Transform::look_at(eye, above, UP), None);
    // Straight down is just as parallel.
    let below = GlobalPoint::new(I24F8::ZERO, I24F8::ZERO, I24F8::from_f64(-10.0));
    assert_eq!(Transform::look_at(eye, below, UP), None);
    // The target coincides with the eye.
    assert_eq!(Transform::look_at(eye, eye, UP), None);
    // `up` is zero-length.
    let ahead = GlobalPoint::new(I24F8::ZERO, I24F8::from_f64(10.0), I24F8::ZERO);
    assert_eq!(Transform::look_at(eye, ahead, Direction::ZERO), None);

    // And a perfectly ordinary aim succeeds.
    assert!(Transform::look_at(eye, ahead, UP).is_some());
}

#[test]
fn looking_at_re_aims_in_place() {
    let start = Transform::new(GlobalPoint::splat(I24F8::from_f64(5.0)), Rotation::IDENTITY);
    let target = GlobalPoint::new(
        I24F8::from_f64(5.0),
        I24F8::from_f64(50.0),
        I24F8::from_f64(5.0),
    );
    let aimed = start.looking_at(target, UP).expect("not parallel");
    assert_eq!(aimed.position(), start.position());
    assert!(aimed.direction_to(target).is_some());
}

#[test]
fn move_towards_never_overshoots() {
    let a = Transform::IDENTITY;
    let far = Transform::new(
        GlobalPoint::splat(I24F8::from_f64(100.0)),
        Rotation::IDENTITY,
    );

    let stepped = a.move_towards(far, I24F8::from_f64(1.0));
    let travelled = a.position().distance(stepped.position()).to_f64();
    assert!((travelled - 1.0).abs() < 0.05, "travelled {travelled}");

    // A step longer than the gap lands exactly on the target position.
    let near = Transform::new(GlobalPoint::splat(I24F8::from_f64(0.5)), Rotation::IDENTITY);
    assert_eq!(
        a.move_towards(near, I24F8::from_f64(100.0)).position(),
        near.position()
    );
    // Already there.
    assert_eq!(
        a.move_towards(a, I24F8::from_f64(1.0)).position(),
        a.position()
    );

    // It always makes progress and never passes the target.
    let mut rng = Rng::new(0x0B50_0002);
    for _ in 0..10_000 {
        let from = common::random_transform(&mut rng, 1000.0);
        let to = common::random_transform(&mut rng, 1000.0);
        let before = from.distance_to(to.position()).to_f64();
        let after = from
            .move_towards(to, I24F8::from_f64(5.0))
            .distance_to(to.position())
            .to_f64();
        assert!(after <= before + 0.05, "moved away: {before} -> {after}");
    }
}

#[test]
fn rotate_towards_never_overshoots() {
    let mut rng = Rng::new(0x0B50_0003);
    let step = Angle32::from_degrees(5.0);
    for _ in 0..5_000 {
        let a = common::random_transform(&mut rng, 100.0);
        let b = common::random_transform(&mut rng, 100.0);

        let before = a.basis().angle_to(b.basis()).to_degrees();
        let moved = a.rotate_towards(b, step);
        // The position is left alone; only the rotation moves.
        assert_eq!(moved.position(), a.position());

        let travelled = a.basis().angle_to(moved.basis()).to_degrees();
        // The coarse codec's own quantum is 0.19 degrees, so allow for it.
        assert!(travelled <= 5.0 + 0.4, "travelled {travelled} degrees");
        assert!(travelled <= before + 0.4);
    }
}

#[test]
fn lerp_is_exact_at_both_ends() {
    let mut rng = Rng::new(0x0B50_0004);
    for _ in 0..10_000 {
        let a = common::random_transform(&mut rng, 1000.0);
        let b = common::random_transform(&mut rng, 1000.0);

        assert_eq!(a.lerp(b, Factor32::ZERO).position(), a.position());
        assert_eq!(a.lerp(b, Factor32::ONE).position(), b.position());

        // The rotation lands on each end to within the codec's own quantum.
        assert!(
            a.lerp(b, Factor32::ZERO)
                .basis()
                .angle_to(a.basis())
                .to_degrees()
                < 0.4
        );
        assert!(
            a.lerp(b, Factor32::ONE)
                .basis()
                .angle_to(b.basis())
                .to_degrees()
                < 0.4
        );

        // And the midpoint is halfway in position, exactly.
        let mid = a.lerp(b, Factor32::from_f64(0.5));
        let expected = a.position().lerp(b.position(), Factor32::from_f64(0.5));
        assert_eq!(mid.position(), expected);
    }
}

#[test]
fn direction_to_is_none_only_on_coincident_points() {
    let t = Transform::IDENTITY;
    assert_eq!(t.direction_to(GlobalPoint::ZERO), None);

    let ahead = GlobalPoint::new(I24F8::ZERO, I24F8::from_f64(5.0), I24F8::ZERO);
    let d = t.direction_to(ahead).expect("distinct");
    assert!((d.y().to_f64() - 1.0).abs() < 1e-6);
    assert!(d.x().to_f64().abs() < 1e-6);

    // One last bit apart is still a direction.
    let barely = GlobalPoint::new(I24F8::from_bits(1), I24F8::ZERO, I24F8::ZERO);
    assert_eq!(
        t.direction_to(barely).map(Direction::x),
        Some(Signed32::MAX)
    );
}

#[test]
fn distance_to_matches_the_point_types_own_distance() {
    let mut rng = Rng::new(0x0B50_0005);
    for _ in 0..10_000 {
        let t = common::random_transform(&mut rng, 1000.0);
        let p = common::random_transform(&mut rng, 1000.0).position();
        assert_eq!(t.distance_to(p), t.position().distance(p));
    }
}

#[test]
fn accessors_report_the_local_axes() {
    let t = Transform::IDENTITY;
    assert_eq!(
        t.forward(),
        Direction::new(Signed32::ZERO, Signed32::MAX, Signed32::ZERO)
    );
    assert_eq!(t.up(), UP);
    assert_eq!(
        t.right(),
        Direction::new(Signed32::MAX, Signed32::ZERO, Signed32::ZERO)
    );

    let moved = t.translated_by(GlobalPoint::splat(I24F8::from_f64(3.0)));
    assert_eq!(moved.position(), GlobalPoint::splat(I24F8::from_f64(3.0)));
    assert_eq!(moved.rotation(), t.rotation());
    assert_eq!(moved.with_position(GlobalPoint::ZERO), t);
}

#[test]
fn the_fine_tier_has_the_same_family() {
    let a = FineTransform::IDENTITY;
    let b = common::random_fine_transform(&mut Rng::new(0x0B50_0006), 100.0);
    assert_eq!(a.lerp(b, Factor32::ZERO).position(), a.position());
    assert_eq!(a.lerp(b, Factor32::ONE).position(), b.position());
    assert!(a.direction_to(b.position()).is_some());
    assert_eq!(a.direction_to(a.position()), None);
    assert_eq!(
        a.rotate_towards(b, Angle32::from_degrees(1.0)).position(),
        a.position()
    );
}

/// The fine tier at the range it exists for.
///
/// Every other case here runs at metres, where `GlobalFinePoint` has range to
/// spare. These run at `1e14` m with the two points on opposite sides of the
/// origin, which is where a saturating difference and a saturating `distance`
/// stop telling the truth -- and where `FineTransform`'s widen-then-subtract is
/// a no-op, because it already *is* the wide type.
#[test]
fn the_fine_tier_is_exact_at_the_far_corners() {
    use corvid_transform::{GlobalFinePoint, I48F16};

    let at = |x: f64, y: f64| {
        FineTransform::new(
            GlobalFinePoint::new(I48F16::from_f64(x), I48F16::from_f64(y), I48F16::ZERO),
            corvid_transform::FineRotation::IDENTITY,
        )
    };

    // A bearing across the whole span: (2e14, 1e14) is 26.565 deg, not 45 deg.
    let here = at(-1.0e14, -5.0e13);
    let there = at(1.0e14, 5.0e13);
    let bearing = here.direction_to(there.position()).expect("distinct");
    let degrees = bearing
        .y()
        .to_f64()
        .atan2(bearing.x().to_f64())
        .to_degrees();
    assert!(
        (degrees - 26.565).abs() < 0.1,
        "bearing {degrees}, expected 26.565"
    );
    assert!(
        here.looking_at(there.position(), UP).is_some(),
        "look_at should aim across the span"
    );

    // A step of 1e13 m must travel 1e13 m, not a multiple of it: the fraction
    // is taken against the true distance, not one clamped at `I48F16::MAX`.
    let from = at(-1.0e14, -1.0e14);
    let to = at(1.0e14, 1.0e14);
    let step = I48F16::from_f64(1.0e13);
    let moved = from.move_towards(to, step);
    let travelled = from.position().distance(moved.position()).to_f64();
    assert!(
        (travelled / 1.0e13 - 1.0).abs() < 0.01,
        "travelled {travelled}, expected 1e13"
    );
}

#[test]
fn the_operation_family_is_available_in_const_context() {
    const T: Transform = Transform::IDENTITY;
    const MOVED: Transform = T.translated_by(GlobalPoint::ZERO);
    const AIMED: Option<Transform> = Transform::looking_to(
        GlobalPoint::ZERO,
        Direction::new(Signed32::ZERO, Signed32::MAX, Signed32::ZERO),
        UP,
    );
    const BLEND: Transform = T.lerp(T, Factor32::from_f64(0.5));
    const DISTANCE: I24F8 = T.distance_to(GlobalPoint::ZERO);

    assert_eq!(MOVED, Transform::IDENTITY);
    assert!(AIMED.is_some());
    assert_eq!(BLEND.position(), GlobalPoint::ZERO);
    assert_eq!(DISTANCE, I24F8::ZERO);
}
