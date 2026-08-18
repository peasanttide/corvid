//! A walker does not fall off the edge of the world, nor climb a scarp.

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
use corvid_nav::{NavCords, NavTriRef, Tune, kinematic_step};

use surface::{quad, scarp, world};

/// The quad's outer edges have nobody on the other side, and a walker sent at
/// one stays on the square.
///
/// Face 0 is `(0,0)`, `(4,0)`, `(0,4)`; its edge 0 is the southern side, where
/// the ground stops. A body walking south for three seconds at one metre per
/// second would be a metre and a half past it if the edge did nothing.
#[test]
fn a_walker_does_not_walk_off_a_boundary_edge() {
    let mesh = quad();
    // Southward at one metre per second, which is `(0.25, 0, 0)` in face 0's
    // axes.
    let start = NavCords {
        tri: NavTriRef(0),
        position: [39_321, 13_107, 0],
        velocity: [4096, 0, 0],
    };

    let after =
        kinematic_step(&mesh, start, I16F16::from_f64(3.0), &Tune::default()).expect("a step");

    assert_eq!(after.tri, NavTriRef(0), "there is nowhere else to be");
    assert!(after.is_inside());
    let landed = world(&mesh, after.tri, after.decode().position);
    assert!(
        landed.y().to_f64() >= -0.05,
        "the ground stops at zero and so does the walker: {landed}"
    );
    assert!(
        landed.z().to_f64() >= surface::RADIUS - 0.05,
        "and it did not fall: {landed}"
    );
}

/// A seam onto a face too steep to walk is a wall as much as a boundary is.
///
/// Face 1 of the scarp is the northern half of the flat shelf; its edge 0 meets
/// the sixty-eight degree face rising out of it, which
/// [`Tune::max_slope`] refuses. A walker sent north bounces off it.
#[test]
fn a_walker_does_not_climb_a_scarp() {
    let mesh = scarp();
    let seam = mesh
        .tri(NavTriRef(1))
        .expect("the northern half of the shelf")
        .edge(0)
        .expect("meets the scarp");
    assert_eq!(seam.next(), NavTriRef(2));
    assert!(
        !seam.is_walkable(),
        "sixty-eight degrees is past the fifty a walker is allowed"
    );

    // Northward at one metre per second, which is `(0.5, 0, 0)` in face 1's
    // axes.
    let start = NavCords {
        tri: NavTriRef(1),
        position: [19_789, 19_789, 0],
        velocity: [8192, 0, 0],
    };
    let after =
        kinematic_step(&mesh, start, I16F16::from_f64(3.0), &Tune::default()).expect("a step");

    assert_eq!(after.tri, NavTriRef(1), "still on the shelf");
    assert!(after.is_inside());
    let landed = world(&mesh, after.tri, after.decode().position);
    assert!(
        landed.y().to_f64() <= 2.05,
        "the shelf ends two metres north and the walker did not pass it: {landed}"
    );
}

/// The other way over the same seam is walkable, because what stops a walker is
/// the slope of the face being stepped onto and not the seam itself.
#[test]
fn the_same_seam_is_walkable_downhill() {
    let mesh = scarp();
    let seam = mesh
        .tri(NavTriRef(2))
        .expect("the scarp")
        .edge(0)
        .expect("meets the shelf");
    assert_eq!(seam.next(), NavTriRef(1));
    assert!(
        seam.is_walkable(),
        "a body may come down what it may not climb"
    );
}
