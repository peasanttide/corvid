//! The two properties the steering holds, frozen, and the rest of the orbit's
//! contract with them.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_camera::Orbit;
use corvid_fixed::{Angle32, Factor32, I24F8, Pitch32};
use corvid_rotation::{FineRotation, Versor};
use corvid_vector::{GlobalPoint, globalpoint};

/// How far apart two versors are, in degrees.
///
/// From the dot product of the two quaternions, which is the cosine of half the
/// angle between the rotations they denote — the absolute value, because a
/// versor and its negation are the same rotation.
fn apart(before: Versor, now: Versor) -> f64 {
    let cosine = before.dot(now).to_f64().abs().min(1.0);
    2.0 * cosine.acos().to_degrees()
}

/// Every representable yaw gives an orientation adjacent to its neighbour's.
///
/// One step of yaw may not turn the camera by more than one step plus what the
/// packing costs, which at `FineRotation`'s 0.0034° is far below the degree
/// this asserts.
///
/// The sweep is exhaustive rather than a spot check. A steering that falls back
/// to [`Versor::IDENTITY`] on the yaws it cannot build is right on the rest, so
/// sampling finds nothing while adjacent yaws sit nearly half a turn apart;
/// [`Orbit`]'s own documentation says which construction has that branch.
#[test]
fn every_yaw_is_adjacent_to_its_neighbour() {
    let mut previous: Option<Versor> = None;
    let mut worst = 0.0f64;

    for index in 0..65_536u32 {
        let mut orbit = Orbit::default();
        // One `Angle16` step, widened into the thirty-two-bit angle `turn` takes.
        orbit.turn(Angle32::from_bits(index << 16), Pitch32::ZERO);
        let now = orbit.orientation();
        if let Some(before) = previous {
            worst = worst.max(apart(before, now));
        }
        previous = Some(now);
    }

    assert!(worst < 1.0, "adjacent yaws differ by up to {worst}°");
}

/// The eye is exactly on the orbit, at every facing.
///
/// The rigid half of what [`Orbit`] documents: the eye is derived from the
/// anchor and the facing every time it is read, so no amount of turning can
/// drift it off the sphere.
#[test]
fn the_eye_is_always_on_the_orbit() {
    let mut orbit = Orbit::new(I24F8::from_f64(8.0));

    for index in 0..1024u32 {
        orbit.turn(Angle32::from_bits(index << 20), Pitch32::ZERO);
        let reach = orbit.eye_position().distance(orbit.anchor);
        assert!(
            (reach.to_f64() - 8.0).abs() < 0.01,
            "step {index}: {reach:?}"
        );
    }
}

/// And at every pitch as well as every yaw, which is the case a yaw-only sweep
/// would miss.
#[test]
fn the_eye_is_on_the_orbit_at_every_pitch() {
    let mut orbit = Orbit::new(I24F8::from_f64(8.0));
    orbit.turn(Angle32::from_turns(0.3), Pitch32::ZERO);

    for step in -64..=64i32 {
        let mut tilted = orbit;
        tilted.turn(Angle32::ZERO, Pitch32::from_turns(f64::from(step) / 512.0));
        let reach = tilted.eye_position().distance(tilted.anchor);
        assert!(
            (reach.to_f64() - 8.0).abs() < 0.01,
            "step {step}: {reach:?}"
        );
    }
}

/// What the packing costs, in turns.
///
/// A [`FineRotation`] is four sixteen-bit components, good to about 0.0034° —
/// which is 9.4e-6 of a turn. The clamp in `turn` is applied to the angle
/// *before* it is packed, so an angle read back out of the packing may sit that
/// far past the limit: 6.6e-7 of a turn, in the case below. This is the
/// tolerance that fact deserves rather than one chosen to make the assertion
/// pass, and it is two orders of magnitude tighter than the amount that would
/// let the camera reach the pole.
const PACKING: f64 = 1e-4;

/// The pitch stops short of the pole, where a yaw has no meaning and the camera
/// would spin on the spot as the player crossed it.
#[test]
fn the_pitch_is_clamped() {
    let limit = Orbit::DEFAULT_PITCH_LIMIT.to_turns();

    let mut orbit = Orbit::default();
    for _ in 0..64 {
        orbit.turn(Angle32::ZERO, Pitch32::from_turns(0.02));
    }
    let (_yaw, pitch) = orbit.angles();
    assert!(pitch.to_turns() <= limit + PACKING, "{pitch:?}");
    // And it did reach the limit rather than stopping somewhere short of it.
    assert!(pitch.to_turns() > limit - PACKING, "{pitch:?}");

    for _ in 0..128 {
        orbit.turn(Angle32::ZERO, Pitch32::from_turns(-0.02));
    }
    let (_yaw, pitch) = orbit.angles();
    assert!(pitch.to_turns() >= -limit - PACKING, "{pitch:?}");
    assert!(pitch.to_turns() < -limit + PACKING, "{pitch:?}");
}

/// A yaw wraps where a pitch clamps, which is the difference between the two
/// types rather than a decision this camera makes.
#[test]
fn the_yaw_wraps() {
    let mut orbit = Orbit::default();
    for _ in 0..8 {
        orbit.turn(Angle32::from_turns(0.125), Pitch32::ZERO);
    }
    // A full turn is back where it started, to within the packing.
    assert!(apart(orbit.orientation(), Versor::IDENTITY) < 0.05);
}

/// Easing moves the anchor and nothing else, so the framing does not depend on
/// how fast the mouse is going.
#[test]
fn easing_moves_the_anchor_only() {
    let mut orbit = Orbit::new(I24F8::from_f64(8.0));
    let before = orbit.orientation();

    orbit.ease_towards(globalpoint(10, 0, 0), Factor32::from_f64(0.25));

    assert_eq!(orbit.orientation(), before);
    assert!(orbit.anchor.x() > I24F8::ZERO);
    assert!(orbit.anchor.x() < I24F8::from_f64(10.0));
}

/// Easing all the way arrives exactly, and easing not at all does not move.
#[test]
fn the_easing_endpoints_are_exact() {
    let mut none = Orbit::default();
    none.ease_towards(globalpoint(10, 0, 0), Factor32::ZERO);
    assert_eq!(none.anchor, GlobalPoint::ZERO);

    let mut all = Orbit::default();
    all.ease_towards(globalpoint(10, 0, 0), Factor32::ONE);
    assert_eq!(all.anchor, globalpoint(10, 0, 0));
}

/// The eye follows the anchor rigidly, so easing the anchor eases the framing.
#[test]
fn the_eye_follows_the_anchor() {
    let mut orbit = Orbit::default();
    let before = orbit.eye_position();
    orbit.ease_towards(globalpoint(10, 0, 0), Factor32::ONE);
    assert_eq!(orbit.eye_position(), before + globalpoint(10, 0, 0));
}

/// The default decodes to the identity exactly, so a run with no mouse on it
/// draws a level camera facing forward.
#[test]
fn the_default_faces_forward() {
    let orbit = Orbit::default();
    assert_eq!(orbit.facing, FineRotation::IDENTITY);
    assert_eq!(orbit.orientation(), Versor::IDENTITY);
    assert_eq!(orbit.anchor, GlobalPoint::ZERO);
    // Facing forward with nothing turned puts the eye directly behind the
    // anchor, at the distance the camera was built with.
    assert_eq!(orbit.eye_position(), globalpoint(0, -8, 0));
}

/// An offset camera sits where its offset says, and the offset rides the
/// facing rather than the world.
///
/// The case `examples/hello` needs: ten metres back and a metre and a half up,
/// which is not expressible as a scalar distance and is the reason the field is
/// a point.
#[test]
fn an_offset_rides_the_facing() {
    let raised = Orbit::default().with_offset(globalpoint(0, -10, 2));
    assert_eq!(raised.eye_position(), globalpoint(0, -10, 2));

    let mut turned = raised;
    turned.turn(Angle32::from_turns(0.25), Pitch32::ZERO);
    // A quarter turn about up carries the eye round to +Y and leaves the rise
    // alone, because the rise is along the axis being turned about.
    let eye = turned.eye_position();
    assert!(eye.y().to_f64().abs() < 0.01, "{eye:?}");
    assert!((eye.z().to_f64() - 2.0).abs() < 0.01, "{eye:?}");
    assert!((eye.x().to_f64() - 10.0).abs() < 0.01, "{eye:?}");
}

/// The distance is derived from the offset rather than stored beside it.
#[test]
fn the_distance_is_the_offset_length() {
    let straight = Orbit::new(I24F8::from_f64(8.0));
    assert!((straight.distance().to_f64() - 8.0).abs() < 0.01);
    assert_eq!(straight.offset, globalpoint(0, -8, 0));

    // Three, four, five.
    let raised = Orbit::default().with_offset(globalpoint(0, -4, 3));
    assert!((raised.distance().to_f64() - 5.0).abs() < 0.01);
}

/// Turning by nothing changes nothing, which is not obvious for a camera whose
/// facing is a read-modify-write through a lossy encoding.
#[test]
fn turning_by_nothing_is_a_no_op() {
    let mut orbit = Orbit::default();
    orbit.turn(Angle32::from_turns(0.3), Pitch32::from_turns(0.1));
    let settled = orbit;

    orbit.turn(Angle32::ZERO, Pitch32::ZERO);
    assert!(
        apart(orbit.orientation(), settled.orientation()) < 0.01,
        "{:?} vs {:?}",
        orbit.facing,
        settled.facing
    );
}
