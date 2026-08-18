//! Walking across seams and along walls, without ever standing still.
//!
//! The bug this file exists for was watched rather than derived: peasants got
//! stuck on triangle edges. Two things were doing it, and both are here as a
//! measurement rather than as a story.
//!
//! The first is a pair of constraints arguing. A body on a tilted face pressed
//! against a wall met a wall standing *upright*, whose normal leans out of the
//! face's plane, so the bounce sent it into the ground; the ground collision
//! slid it back into the wall; and because a body already touching a boundary
//! is at distance zero from it, both events took no time at all. Eight of those
//! spent the step's whole budget without advancing the clock, and the body
//! finished the tick exactly where it started -- every tick, forever.
//!
//! The second is quantisation. A position was eight bits across a triangle, so
//! anybody moving less than half a code in a tick was rounded back to where
//! they were, and a slow walker on a large triangle never moved at all.

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
use corvid_nav::{NavCords, NavMesh, NavState, NavTriRef, Tune, kinematic_step};
use corvid_vector::FinePoint;

use surface::{bank, inert, metres, sheet};

/// The tick a walk is taken in, and the speed it is taken at.
const STEP: f64 = 0.25;
const SPEED: f64 = 1.0;

/// Where in the world a set of coordinates is, as metres east, north and up.
fn place(mesh: &NavMesh, cords: NavCords) -> [f64; 3] {
    let tri = mesh.tri(cords.tri).expect("a triangle of this mesh");
    let [x, y, z] = tri.ecef(cords.decode().position).to_array();
    [x.to_f64(), y.to_f64(), z.to_f64() - surface::RADIUS]
}

/// How far apart two of those are.
fn gap(from: [f64; 3], to: [f64; 3]) -> f64 {
    let [dx, dy, dz] = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    dz.hypot(dx.hypot(dy))
}

/// A body at `at`, moving at `speed` along the given world heading.
fn walker(mesh: &NavMesh, at: [f64; 2], heading: f64, speed: f64) -> NavCords {
    let start = mesh
        .locate(metres(at[0], at[1], 0.0))
        .expect("the fixture has ground there");
    let tri = mesh.tri(start.tri).expect("a triangle of this mesh");
    let world = FinePoint::new(
        I16F16::from_f64(speed * heading.cos()),
        I16F16::from_f64(speed * heading.sin()),
        I16F16::ZERO,
    );
    NavCords::encode(NavState {
        tri: start.tri,
        position: start.decode().position,
        velocity: tri.ecef_to_local().apply(world),
    })
}

/// Walks a body for `steps` ticks and answers how far it moved each one.
fn walk(mesh: &NavMesh, mut cords: NavCords, steps: usize, tune: &Tune) -> Vec<f64> {
    let mut moved = Vec::with_capacity(steps);
    let mut before = place(mesh, cords);
    for _ in 0..steps {
        cords = kinematic_step(mesh, cords, I16F16::from_f64(STEP), tune).expect("a step");
        let after = place(mesh, cords);
        moved.push(gap(before, after));
        before = after;
    }
    moved
}

/// **The reproduction.** A body walking into the wall at the edge of a tilted
/// square keeps walking, whatever the tilt is.
///
/// Every state is swept: both triangles of the square, every position along all
/// three of their edges, and a fan of velocities across the whole encoding.
/// None of them may spend two consecutive ticks in the same place while it
/// still has a speed to walk at. Before the wall was taken into the plane of
/// the face, every slope from 45 degrees up had hundreds that did, and they
/// stayed stuck for good rather than for a tick.
#[test]
fn nobody_stands_still_against_a_wall_on_a_slope() {
    let tune = Tune::default();
    let step = I16F16::from_f64(0.1);
    let mut worst = (f64::MAX, 0.0, 0u32, [0u16; 3], [0i16; 3]);
    for tenth in 0..=12u32 {
        let rise = f64::from(tenth) * 0.5;
        let mesh = bank(rise);
        for tri in 0..2u32 {
            for along in (1000..=64_000u16).step_by(3100) {
                for east in (-2048..=2048i16).step_by(256) {
                    for north in (-2048..=2048i16).step_by(256) {
                        for position in [[0, along, 0], [along, 0, 0], [along, 65_535 - along, 0]] {
                            let start = NavCords {
                                tri: NavTriRef(tri),
                                position,
                                velocity: [east, north, 0],
                            };
                            let pace = speed(&mesh, start);
                            if !start.is_inside() || pace < 0.2 {
                                continue;
                            }
                            let mut cords = start;
                            let mut here = place(&mesh, cords);
                            let mut travelled = 0.0;
                            for _ in 0..4 {
                                cords = kinematic_step(&mesh, cords, step, &tune).expect("a step");
                                let next = place(&mesh, cords);
                                travelled += gap(here, next);
                                here = next;
                            }
                            let ratio = travelled / (pace * 0.4);
                            if ratio < worst.0 {
                                worst = (ratio, rise, tri, position, start.velocity);
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        worst.0 > 0.05,
        "the least ground anybody covered was {:.4} of the {:.3} m/s it was walking at,          on a {} m rise at {:?} of {} moving {:?}",
        worst.0,
        worst.1,
        worst.1,
        worst.3,
        worst.2,
        worst.4
    );
}

/// How fast a body is going across the ground, in metres per second.
fn speed(mesh: &NavMesh, cords: NavCords) -> f64 {
    let tri = mesh.tri(cords.tri).expect("a triangle of this mesh");
    let moved = tri.local_to_ecef().apply(cords.decode().velocity);
    moved.y().to_f64().hypot(moved.x().to_f64())
}

/// A body walking in a straight line over a seamed sheet keeps going, whatever
/// angle it takes the seams at.
///
/// The sheet's seams run east, north and along the diagonal, so the headings
/// here include four that are exactly parallel to a seam, sixteen that are a
/// hair off one, and twenty-four spread around the circle. Every one of them
/// has to cover the same ground: `SPEED * STEP` a tick, every tick, with no
/// tick spent standing still on a seam.
#[test]
fn a_walk_keeps_moving_at_every_angle() {
    let mesh = sheet(8, 2.0);
    let expected = SPEED * STEP;
    let mut headings: Vec<f64> = (0..24)
        .map(|k| f64::from(k) * core::f64::consts::TAU / 24.0)
        .collect();
    for near in [0.0f64, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0] {
        for offset in [-0.35, -0.02, 0.02, 0.35] {
            headings.push((near + offset).to_radians());
        }
    }

    for heading in headings {
        let cords = walker(&mesh, [8.0, 8.0], heading, SPEED);
        let moved = walk(&mesh, cords, 16, &inert());
        for (tick, step) in moved.iter().enumerate() {
            assert!(
                *step > expected * 0.6,
                "heading {:.2} degrees stalled on tick {tick}: {step:.4} m of {expected:.4}, \
                 whole walk {moved:?}",
                heading.to_degrees()
            );
        }
        let total: f64 = moved.iter().sum();
        assert!(
            total > expected * 16.0 * 0.9,
            "heading {:.2} degrees covered {total:.3} m of {:.3}",
            heading.to_degrees(),
            expected * 16.0
        );
    }
}

/// And a body aimed exactly at a vertex where six triangles meet walks through
/// it rather than spinning round the fan.
#[test]
fn a_walk_through_a_vertex_arrives() {
    let mesh = sheet(8, 2.0);
    let expected = SPEED * STEP;
    for octant in 0..8 {
        let heading = f64::from(octant) * core::f64::consts::TAU / 8.0;
        let from = [8.0 - 2.0 * heading.cos(), 8.0 - 2.0 * heading.sin()];
        let cords = walker(&mesh, from, heading, SPEED);
        let moved = walk(&mesh, cords, 12, &inert());
        for (tick, step) in moved.iter().enumerate() {
            assert!(
                *step > expected * 0.6,
                "bearing {:.1} degrees through the fan stalled on tick {tick}: {step:.4} m, \
                 whole walk {moved:?}",
                heading.to_degrees()
            );
        }
    }
}

/// A crossing preserves speed: what goes into a seam comes out of it.
#[test]
fn speed_survives_a_crossing() {
    let mesh = sheet(8, 2.0);
    let expected = SPEED * STEP;
    for k in 0..16 {
        let heading = f64::from(k) * core::f64::consts::TAU / 16.0;
        let cords = walker(&mesh, [8.0, 8.0], heading, SPEED);
        let moved = walk(&mesh, cords, 16, &inert());
        for (tick, step) in moved.iter().enumerate() {
            assert!(
                (step - expected).abs() < expected * 0.1,
                "heading {:.1} degrees moved {step:.4} m on tick {tick} rather than {expected:.4}",
                heading.to_degrees()
            );
        }
    }
}

/// Somebody strolling at five centimetres a second moves too, which is what a
/// sixteen-bit coordinate bought.
///
/// Eight bits across a two-metre triangle is 7.8 mm, and a tick of this walk is
/// 12.5 mm, so the old encoding rounded roughly every other tick back to where
/// it started and a slower walker never moved at all. The assertion is the one
/// that matters for a crowd: over sixteen ticks the ground covered is the
/// ground a straight line says.
#[test]
fn a_slow_walker_is_not_rounded_to_a_standstill() {
    let mesh = sheet(8, 2.0);
    let creep = 0.05;
    for k in 0..12 {
        let heading = f64::from(k) * core::f64::consts::TAU / 12.0;
        let cords = walker(&mesh, [8.0, 8.0], heading, creep);
        let moved = walk(&mesh, cords, 16, &inert());
        let total: f64 = moved.iter().sum();
        assert!(
            total > creep * STEP * 16.0 * 0.8,
            "a creep at {:.1} degrees covered {total:.4} m of {:.4}",
            heading.to_degrees(),
            creep * STEP * 16.0
        );
    }
}
