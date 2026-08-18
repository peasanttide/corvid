//! Sliding below the incidence threshold and bouncing above it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

#[allow(
    dead_code,
    unreachable_pub,
    reason = "one fixture module serves every test file, and each file uses the surfaces it needs"
)]
mod surface;

use corvid_fixed::I16F16;
use corvid_nav::{NavCords, NavMesh, NavState, NavTriRef, Tune, calc_collision_vs_plane};
use corvid_vector::FinePoint;

use surface::{ramp, world_velocity};

/// The speed at which a velocity approaches or leaves the face it is over.
///
/// Negative into the face. This is what the slide/bounce decision is about, and
/// reading it back through the world axes rather than the local ones is what
/// makes the assertion independent of the frame the code works in.
fn normal_speed(mesh: &NavMesh, tri: NavTriRef, local: FinePoint) -> f64 {
    let [east, north, up] = world_velocity(mesh, tri, local);
    let normal = mesh.tri(tri).expect("a triangle").normal().to_array();
    east * normal[0].to_f64() + north * normal[1].to_f64() + up * normal[2].to_f64()
}

/// A body dropped onto the ramp is standing on it, one metre up.
///
/// The local axes of face 0 are `(0,-4,-2)`, `(4,-4,-2)` and the up, so a
/// velocity straight down at four metres per second is `(0, 0, -4)` in them and
/// encodes exactly.
fn dropped(velocity: [i16; 3]) -> NavCords {
    NavCords {
        tri: NavTriRef(0),
        position: [21_845, 21_845, 8224],
        velocity,
    }
}

/// A near-vertical hit on a 26.57 degree ramp comes in at 63 degrees, which is
/// over the 45 the tune allows, so it bounces.
///
/// Bouncing means the normal component reverses and keeps
/// [`Tune::restitution`] of itself: 3.578 m/s into the face becomes a quarter
/// of that back out of it.
#[test]
fn a_steep_hit_bounces() {
    let mesh = ramp();
    let tune = Tune::default();
    let state = dropped([0, 0, -8192]).decode();

    let before = normal_speed(&mesh, state.tri, state.velocity);
    assert!(
        (before + 3.578).abs() < 0.02,
        "four metres per second straight down is 3.578 into a 26.57 degree face, not {before}"
    );

    let event = calc_collision_vs_plane(
        mesh.tri(state.tri).expect("a triangle"),
        state,
        I16F16::from_f64(1.0),
        &tune,
    )
    .expect("a body falling onto the face it is over hits it");

    let after = normal_speed(&mesh, event.state.tri, event.state.velocity);
    assert!(
        (after - 0.894).abs() < 0.05,
        "a quarter of 3.578 back out of the face, not {after}"
    );
}

/// A grazing hit comes in at 8.5 degrees, which is under the threshold, so it
/// slides: the normal component goes to nothing and the body keeps the speed it
/// had along the face.
#[test]
fn a_grazing_hit_slides() {
    let mesh = ramp();
    let tune = Tune::default();
    // Three metres per second east and half a metre per second down, which is
    // `(-0.75, 0.75, -0.5)` in face 0's axes and encodes exactly.
    let state = dropped([-12_288, 12_288, -1024]).decode();

    let before = normal_speed(&mesh, state.tri, state.velocity);
    assert!(
        before < -0.4 && before > -0.5,
        "gently into the face: {before}"
    );

    // Two seconds to fall the metre it is above the face at half a metre per
    // second, so the window has to be wider than that or there is no hit to
    // resolve.
    let event = calc_collision_vs_plane(
        mesh.tri(state.tri).expect("a triangle"),
        state,
        I16F16::from_f64(4.0),
        &tune,
    )
    .expect("a hit");

    let after = normal_speed(&mesh, event.state.tri, event.state.velocity);
    assert!(after.abs() < 0.05, "nothing left across the face: {after}");

    let [east, _, _] = world_velocity(&mesh, event.state.tri, event.state.velocity);
    assert!(
        (east - 3.0).abs() < 0.05,
        "the three metres per second along the face survive a slide: {east}"
    );
}

/// The threshold is the thing being tested, not the geometry: the same grazing
/// hit against a tune that calls eight degrees steep bounces instead.
#[test]
fn the_threshold_is_what_decides() {
    let mesh = ramp();
    let strict = Tune {
        slide_angle: corvid_fixed::Angle16::from_degrees(1.0),
        ..Tune::default()
    };
    let state = dropped([-12_288, 12_288, -1024]).decode();

    let event = calc_collision_vs_plane(
        mesh.tri(state.tri).expect("a triangle"),
        state,
        I16F16::from_f64(4.0),
        &strict,
    )
    .expect("a hit");

    let after = normal_speed(&mesh, event.state.tri, event.state.velocity);
    assert!(after > 0.05, "away from the face now: {after}");
}

/// Sliding is what turns gravity into downhill motion.
///
/// A body resting on the ramp with nothing pushing it is moving south -- down
/// the slope -- a second later, because each substep's gravity is projected
/// onto the face rather than cancelled by it.
#[test]
fn a_body_at_rest_on_a_ramp_slides_downhill() {
    let mesh = ramp();
    let tune = Tune::default();
    let mut cords = NavCords::centred(NavTriRef(0));
    // Twelve twentieths of a second, which is as long as the body has before it
    // reaches the southern edge of the square and the wall there stops it.
    for _ in 0..12 {
        cords = corvid_nav::kinematic_step(&mesh, cords, I16F16::from_f64(0.05), &tune)
            .expect("a step");
    }

    let state: NavState = cords.decode();
    let [east, north, _] = world_velocity(&mesh, state.tri, state.velocity);
    assert!(north < -1.5, "downhill is south, and it is moving: {north}");
    assert!(east.abs() < 0.2, "and not sideways: {east}");
}
