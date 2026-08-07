//! Walking in the camera's frame rather than the world's.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_camera::FirstPerson;
use corvid_fixed::{Angle32, I24F8, Pitch32};
use corvid_vector::{Direction, GlobalPoint, globalpoint};

/// Two metres, spelled once.
const fn two() -> I24F8 {
    I24F8::from_f64(2.0)
}

/// How near two lengths have to be. A step is rotated through a packed
/// rotation, so it arrives within the packing rather than exactly.
const CLOSE: f64 = 0.01;

#[track_caller]
fn near(left: I24F8, right: f64, what: &str) {
    assert!(
        (left.to_f64() - right).abs() < CLOSE,
        "{what}: {left:?} vs {right}"
    );
}

/// Walking forward from the default facing goes +Y.
#[test]
fn walking_forward_goes_forward() {
    let mut person = FirstPerson::new(GlobalPoint::ZERO);
    person.walk(two(), I24F8::ZERO, I24F8::ZERO);
    assert_eq!(person.position, globalpoint(0, 2, 0));
}

/// Walking right goes +X, and up goes +Z. The argument order is forward,
/// right, up, which is the order the field of view is described in.
#[test]
fn the_three_axes_are_where_they_say() {
    let mut person = FirstPerson::new(GlobalPoint::ZERO);
    person.walk(I24F8::ZERO, two(), I24F8::ZERO);
    assert_eq!(person.position, globalpoint(2, 0, 0));

    let mut climber = FirstPerson::new(GlobalPoint::ZERO);
    climber.walk(I24F8::ZERO, I24F8::ZERO, two());
    assert_eq!(climber.position, globalpoint(0, 0, 2));
}

/// Turning first means walking in the camera's frame rather than the world's,
/// which is the whole difference between a controller and an offset.
#[test]
fn walking_is_in_the_camera_frame() {
    let mut person = FirstPerson::new(GlobalPoint::ZERO);
    person.turn(Angle32::from_turns(0.25), Pitch32::ZERO);
    person.walk(two(), I24F8::ZERO, I24F8::ZERO);

    near(person.position.x(), -2.0, "x");
    near(person.position.y(), 0.0, "y");
    near(person.position.z(), 0.0, "z");
}

/// Looking down and walking forward does not walk into the floor: that is what
/// `walk_level` is for, and it is the one a character controller calls.
#[test]
fn walking_level_ignores_the_pitch() {
    let mut person = FirstPerson::new(GlobalPoint::ZERO);
    person.turn(Angle32::ZERO, Pitch32::from_turns(-0.15));
    person.walk_level(two(), I24F8::ZERO);

    assert_eq!(person.position.z(), I24F8::ZERO);
    near(person.position.y(), 2.0, "y");
}

/// `walk` with the same pitch does dip, which is the pair the previous test is
/// half of — a flying camera, which is what that method is for.
#[test]
fn walking_unlevelled_does_dip() {
    let mut person = FirstPerson::new(GlobalPoint::ZERO);
    person.turn(Angle32::ZERO, Pitch32::from_turns(-0.15));
    person.walk(two(), I24F8::ZERO, I24F8::ZERO);
    assert!(person.position.z() < I24F8::ZERO, "{:?}", person.position);
}

/// A level walk still turns with the yaw, which is what makes it a walk rather
/// than a translation.
#[test]
fn walking_level_still_turns() {
    let mut person = FirstPerson::new(GlobalPoint::ZERO);
    person.turn(Angle32::from_turns(0.25), Pitch32::from_turns(-0.15));
    person.walk_level(two(), I24F8::ZERO);

    near(person.position.x(), -2.0, "x");
    assert_eq!(person.position.z(), I24F8::ZERO);
}

/// The pitch stops short of straight up, for the reason an orbit's does.
#[test]
fn the_pitch_is_clamped() {
    let limit = FirstPerson::DEFAULT_PITCH_LIMIT.to_turns();
    let mut person = FirstPerson::new(GlobalPoint::ZERO);
    for _ in 0..64 {
        person.turn(Angle32::ZERO, Pitch32::from_turns(0.02));
    }
    let (_yaw, pitch) = person.angles();
    assert!(pitch.to_turns() <= limit + 1e-4, "{pitch:?}");
}

/// A default walker is at the origin facing forward, which is the pose a view
/// draws its first frame from.
#[test]
fn the_default_faces_forward() {
    let person = FirstPerson::default();
    assert_eq!(person.position, GlobalPoint::ZERO);
    assert_eq!(person.forward(), Direction::Y);
}

/// Walking is cumulative rather than absolute, which is what makes it a step.
#[test]
fn walking_accumulates() {
    let mut person = FirstPerson::new(GlobalPoint::ZERO);
    person.walk(two(), I24F8::ZERO, I24F8::ZERO);
    person.walk(two(), I24F8::ZERO, I24F8::ZERO);
    assert_eq!(person.position, globalpoint(0, 4, 0));
}
