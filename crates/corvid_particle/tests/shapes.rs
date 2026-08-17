//! Every shape puts its particles where it says and points them where it says.
//!
//! The direction is not on an [`Instance`](corvid_particle::Instance), so it is
//! measured rather than read: with no gravity and no drag a particle moves at
//! the velocity it was born with, so one step of a known duration divided by
//! that duration is the direction and the speed it left at. That is the same
//! way a renderer would have to find it, which is the point of checking it this
//! way.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_glm::Vec3;
use corvid_particle::{Emitter, Range, Shape, System};

/// The step the velocities are measured over.
const DT: f32 = 1.0 / 64.0;

/// Where a burst of particles was born, and which way each of them left.
///
/// The speed is one metre a second, so the second vector of each pair is a unit
/// vector -- which is what the shapes are being checked as.
fn thrown(at: Vec3, shape: Shape, count: u32) -> Vec<(Vec3, Vec3)> {
    let mut system = System::new(1024, 1789);
    let mut emitter = Emitter::new(at, shape);
    emitter.speed = Range::exactly(1.0);
    emitter.lifetime = Range::exactly(100.0);
    let id = system.add(emitter);
    system.burst(id, count).expect("the emitter is live");

    let born: Vec<Vec3> = system
        .instances()
        .map(|instance| Vec3::from(instance.position))
        .collect();
    system.step(DT);
    system
        .instances()
        .map(|instance| Vec3::from(instance.position))
        .zip(born)
        .map(|(moved, born)| (born, (moved - born) / DT))
        .collect()
}

/// A ring lies flat in its plane, at its radius, pointing outward.
///
/// The shockwave. With the world's up as the normal the whole of it is on the
/// ground, and every particle leaves along its own radius rather than along a
/// shared direction -- which is the difference between a ring that expands and
/// a puff that drifts.
#[test]
fn a_ring_expands_in_its_plane() {
    let at = Vec3::new(10.0, -4.0, 5.0);
    for (born, direction) in thrown(
        at,
        Shape::Ring {
            normal: Vec3::new(0.0, 0.0, 1.0),
            radius: 2.0,
        },
        128,
    ) {
        let offset = born - at;
        assert!(offset.z.abs() < 1e-5, "off the ground plane: {}", offset.z);
        assert!((offset.norm() - 2.0).abs() < 1e-4, "off the circle");
        assert!(direction.z.abs() < 1e-5, "leaving the plane");
        // Outward means the direction is the radius, normalized.
        let outward = offset / offset.norm();
        assert!((direction - outward).norm() < 1e-4, "not outward");
    }
}

/// A cone with no spread is its axis, exactly, which is what makes a closed
/// form testable in `tests/motion.rs`.
#[test]
fn a_cone_of_no_spread_is_its_axis() {
    let axis = Vec3::new(0.0, 0.0, 1.0);
    for (born, direction) in thrown(Vec3::zeros(), Shape::Cone { axis, spread: 0.0 }, 32) {
        assert!(born.norm() < 1e-6, "a cone is born at a point");
        assert!((direction - axis).norm() < 1e-5, "{direction:?}");
    }
}

/// And a cone with a spread stays inside it, and fills it.
#[test]
fn a_cone_keeps_within_its_spread() {
    let axis = Vec3::new(1.0, 1.0, 0.0).normalize();
    let spread = 0.4_f32;
    let limit = spread.cos();
    let mut widest = 1.0_f32;
    for (_, direction) in thrown(Vec3::zeros(), Shape::Cone { axis, spread }, 512) {
        let alignment = direction.dot(&axis);
        assert!(alignment >= limit - 1e-4, "outside the cone: {alignment}");
        widest = widest.min(alignment);
    }
    assert!(
        widest < limit + 0.01,
        "nothing near the rim of the cone: {widest}"
    );
}

/// A sphere is a ball around the emitter, thrown outward.
#[test]
fn a_sphere_is_a_ball() {
    let at = Vec3::new(-3.0, 2.0, 1.0);
    let radius = 0.5;
    for (born, direction) in thrown(at, Shape::Sphere { radius }, 256) {
        let offset = born - at;
        assert!(offset.norm() <= radius + 1e-5, "outside the ball");
        assert!(
            (direction.norm() - 1.0).abs() < 1e-4,
            "not a unit direction"
        );
        if offset.norm() > 1e-4 {
            let outward = offset / offset.norm();
            assert!((direction - outward).norm() < 1e-4, "not outward");
        }
    }
}

/// A volume is a box around the emitter, thrown every way.
#[test]
fn a_volume_is_a_box() {
    let at = Vec3::new(0.0, 0.0, 8.0);
    let half = Vec3::new(4.0, 0.5, 2.0);
    let mut reached = Vec3::zeros();
    for (born, direction) in thrown(at, Shape::Volume { half_extent: half }, 512) {
        let offset = born - at;
        assert!(offset.x.abs() <= half.x + 1e-5, "outside the box");
        assert!(offset.y.abs() <= half.y + 1e-5, "outside the box");
        assert!(offset.z.abs() <= half.z + 1e-5, "outside the box");
        assert!(
            (direction.norm() - 1.0).abs() < 1e-4,
            "not a unit direction"
        );
        reached = reached.sup(&offset.abs());
    }
    // And fills it: five hundred draws get within a tenth of every face.
    assert!(reached.x > half.x * 0.9, "never reached the ends");
    assert!(reached.y > half.y * 0.9, "never reached the sides");
    assert!(reached.z > half.z * 0.9, "never reached the top");
}

/// A point emitter throws in every direction and no direction is preferred.
///
/// The check is the mean: a thousand unit vectors sampled uniformly over a
/// sphere average out to nearly nothing, where a thousand sampled by two
/// uniform angles would pile up at the poles and would not.
#[test]
fn a_point_is_uniform_over_the_sphere() {
    let thrown = thrown(Vec3::zeros(), Shape::Point, 1024);
    let count = thrown.len();
    assert_eq!(count, 1024);
    let mut mean = Vec3::zeros();
    for (born, direction) in thrown {
        assert!(born.norm() < 1e-6, "a point is a point");
        mean += direction;
    }
    let bias = mean.norm() / 1024.0;
    assert!(bias < 0.1, "the directions lean somewhere: {bias}");
}
