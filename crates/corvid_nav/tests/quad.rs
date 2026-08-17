//! A walk across the seam of a flat quad, against closed-form arithmetic.

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
use corvid_nav::{NavCords, NavTriRef, kinematic_step};

use surface::{apart, inert, metres, quad, world, world_velocity};

/// The two halves of the square meet along one seam and nowhere else.
#[test]
fn a_quad_has_one_seam() {
    let mesh = quad();
    assert_eq!(mesh.len(), 2);
    assert_eq!(mesh.neighbours(NavTriRef(0)).count(), 1);
    assert_eq!(mesh.neighbours(NavTriRef(1)).count(), 1);
    assert_eq!(
        mesh.neighbours(NavTriRef(0)).next(),
        Some(NavTriRef(1)),
        "the seam is edge 1 of face 0, between vertices 1 and 2"
    );
}

/// A body crossing the seam lands where the geometry says it should.
///
/// Face 0 is `(0,0)`, `(4,0)`, `(0,4)` and its local `x` and `y` weight the
/// first two of those, so a point at barycentric `(0.6, 0.2)` is at
/// `(0.8, 0.8)` on the ground. Three seconds of one metre per second eastward
/// puts it at `(3.8, 0.8)`, which is past the diagonal and therefore in face 1.
/// Both numbers are arithmetic, not a recording of what the code did.
#[test]
fn a_walk_across_the_seam_lands_where_arithmetic_says() {
    let mesh = quad();
    let tune = inert();

    // Eastward at one metre per second, expressed in face 0's local axes: the
    // frame's first two columns are `(0,-4)` and `(4,-4)`, so a metre east is
    // a quarter of the second axis less a quarter of the first.
    let start = NavCords {
        tri: NavTriRef(0),
        position: [153, 51, 0],
        velocity: [-16, 16, 0],
    };
    assert!(
        apart(
            world(&mesh, start.tri, start.decode().position),
            metres(0.8, 0.8, 0.0)
        ) < 0.01,
        "the fixture's own starting point has to be where the arithmetic says too"
    );

    let after = kinematic_step(&mesh, start, I16F16::from_f64(3.0), &tune).expect("a step");

    assert_eq!(
        after.tri,
        NavTriRef(1),
        "three metres east crosses the seam"
    );
    let landed = world(&mesh, after.tri, after.decode().position);
    assert!(
        apart(landed, metres(3.8, 0.8, 0.0)) < 0.05,
        "landed at {landed} rather than 3.8 m east, within a position code and a half"
    );

    // The velocity has to survive the seam as well as the position: it is a
    // different matrix in face 1's frame and the same metre per second east.
    let [east, north, up] = world_velocity(&mesh, after.tri, after.decode().velocity);
    assert!((east - 1.0).abs() < 0.05, "east {east}");
    assert!(north.abs() < 0.05, "north {north}");
    assert!(up.abs() < 0.05, "up {up}");
}

/// Stopping short of the seam stays on the near side of it.
///
/// The same walk for two seconds reaches `(2.8, 0.8)`, which is inside face 0,
/// and a step that crossed anyway would be crossing on the strength of
/// something other than the arithmetic.
#[test]
fn a_walk_that_stops_short_does_not_cross() {
    let mesh = quad();
    let start = NavCords {
        tri: NavTriRef(0),
        position: [153, 51, 0],
        velocity: [-16, 16, 0],
    };
    let after = kinematic_step(&mesh, start, I16F16::from_f64(2.0), &inert()).expect("a step");

    assert_eq!(after.tri, NavTriRef(0));
    let landed = world(&mesh, after.tri, after.decode().position);
    assert!(
        apart(landed, metres(2.8, 0.8, 0.0)) < 0.05,
        "landed at {landed}"
    );
}
