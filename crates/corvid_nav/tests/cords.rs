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

/// The twelve bytes are twelve bytes, and they are the position and the
/// velocity.
#[test]
fn a_position_and_a_velocity_are_twelve_bytes() {
    assert_eq!(size_of::<[u16; 3]>() + size_of::<[i16; 3]>(), 12);
    let cords = NavCords {
        tri: NavTriRef(9),
        position: [1, 2, 3],
        velocity: [-4, 5, -6],
    };
    assert_eq!(
        cords.local_bytes(),
        [1, 0, 2, 0, 3, 0, 252, 255, 5, 0, 250, 255]
    );
}

/// Every code a `NavCords` can hold comes back as itself.
///
/// Exhaustive over each of the position's three codes and over every velocity
/// code, because a coordinate that drifted by one code per tick would drift and
/// no sampled test would find it. The pair of barycentric codes is swept along
/// its own axes and along the diagonal the repair in
/// [`NavCords::encode`](corvid_nav::NavCords::encode) acts on, rather than over
/// all four billion combinations of the two.
#[test]
fn every_code_round_trips() {
    for code in 0..=65_535u16 {
        for position in [
            [code, 0, code],
            [0, code, 65_535 - code],
            [code / 2, code / 2, code],
        ] {
            let cords = NavCords {
                tri: NavTriRef(0),
                position,
                velocity: [-32_767, 0, 32_767],
            };
            assert_eq!(
                NavCords::encode(cords.decode()),
                cords,
                "position {position:?}"
            );
        }
    }

    for code in -32_767..=32_767i16 {
        let cords = NavCords {
            tri: NavTriRef(0),
            position: [10_000, 10_000, 10_000],
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
    for first in [0u16, 1, 32_768, 50_000, 65_535] {
        for second in [0u16, 1, 32_768, 50_000, 65_535] {
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

    for first in [0u16, 8192, 16_384, 32_768, 51_200] {
        for second in [0u16, 4096, 14_080] {
            let start = NavCords {
                tri: NavTriRef(0),
                position: [first, second, 6144],
                velocity: [1792, -1792, 768],
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
