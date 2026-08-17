//! What survives encoding, a seam, and decoding again.

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

use corvid_nav::{NavCords, NavTriRef};

use surface::{apart, quad, world};

/// The six bytes are six bytes, and they are the position and the velocity.
#[test]
fn a_position_and_a_velocity_are_six_bytes() {
    assert_eq!(size_of::<[u8; 3]>() + size_of::<[i8; 3]>(), 6);
    let cords = NavCords {
        tri: NavTriRef(9),
        position: [1, 2, 3],
        velocity: [-4, 5, -6],
    };
    assert_eq!(cords.local_bytes(), [1, 2, 3, 252, 5, 250]);
}

/// Every code a `NavCords` can hold comes back as itself.
///
/// Exhaustive over both bytes of the position and both ends of the velocity,
/// because a coordinate that drifted by one code per tick would drift a metre a
/// minute and no sampled test would find it.
#[test]
fn every_code_round_trips() {
    for first in 0..=255u8 {
        for second in 0..=(255 - first) {
            let cords = NavCords {
                tri: NavTriRef(0),
                position: [first, second, first],
                velocity: [-127, 0, 127],
            };
            assert_eq!(
                NavCords::encode(cords.decode()),
                cords,
                "position [{first}, {second}]"
            );
        }
    }

    for code in -127..=127i8 {
        let cords = NavCords {
            tri: NavTriRef(0),
            position: [40, 40, 40],
            velocity: [code, -code, code],
        };
        assert_eq!(NavCords::encode(cords.decode()), cords, "velocity {code}");
    }
}

/// Encoding repairs the one thing rounding can break.
///
/// Two barycentric codes that sum past 255 would name a point outside the
/// triangle, so they cannot both be kept, and which one gives way is fixed
/// rather than whichever the arithmetic reached first.
#[test]
fn an_encoded_position_is_always_inside() {
    let mesh = quad();
    for first in [0u8, 1, 128, 200, 255] {
        for second in [0u8, 1, 128, 200, 255] {
            let outside = NavCords {
                tri: NavTriRef(0),
                position: [first, second, 0],
                velocity: [0; 3],
            };
            let repaired = NavCords::encode(outside.decode());
            assert!(
                repaired.is_inside(),
                "[{first}, {second}] came back as {:?}",
                repaired.position
            );
            // And the repaired point is a point on the surface, which is what
            // being inside is for.
            assert!(mesh.tri(repaired.tri).is_some());
        }
    }
}

/// A position carried across a seam and back is the position it started as.
///
/// The two maps are inverses of one another only as far as the arithmetic
/// allows, so what this pins down is that the round trip stays inside a single
/// position code -- which is what makes a body that paces back and forth over a
/// seam stay where it is instead of walking away.
#[test]
fn a_crossing_and_a_crossing_back_leave_a_position_where_it_was() {
    let mesh = quad();
    let out = mesh
        .tri(NavTriRef(0))
        .expect("face 0")
        .edge(1)
        .expect("the seam");
    let back = mesh
        .tri(NavTriRef(1))
        .expect("face 1")
        .edge(1)
        .expect("the same seam from the other side");
    assert_eq!(out.next(), NavTriRef(1));
    assert_eq!(back.next(), NavTriRef(0));

    for first in [0u8, 32, 64, 128, 200] {
        for second in [0u8, 16, 55] {
            let start = NavCords {
                tri: NavTriRef(0),
                position: [first, second, 24],
                velocity: [7, -7, 3],
            };
            let here = start.decode();
            let there = out.local_to_next().apply(here.position);
            let again = back.local_to_next().apply(there);

            let started = world(&mesh, NavTriRef(0), here.position);
            let returned = world(&mesh, NavTriRef(0), again);
            assert!(
                apart(started, returned) < 0.01,
                "[{first}, {second}] left from {started} and came back to {returned}"
            );

            // And a velocity, which takes the linear part alone.
            let carried = back
                .vel_to_next()
                .apply(out.vel_to_next().apply(here.velocity));
            assert_eq!(
                NavCords::encode(corvid_nav::NavState {
                    tri: NavTriRef(0),
                    position: again,
                    velocity: carried,
                })
                .velocity,
                start.velocity
            );
        }
    }
}
