//! The box: what it holds, what it merges to, and what a ray does to it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::I24F8;
use corvid_shape::{Aabb, Ray};
use corvid_vector::{Direction, GlobalPoint, globalpoint};

/// Two metres across, centred on the origin.
fn unit() -> Aabb {
    Aabb::new(globalpoint(-1, -1, -1), globalpoint(1, 1, 1))
}

#[test]
fn a_box_contains_its_own_centre() {
    assert!(unit().contains(GlobalPoint::ZERO));
    assert!(!unit().contains(globalpoint(2, 0, 0)));
}

/// The boundary is inside. A half-open box makes a point on a face shared by
/// two adjacent cells belong to neither, which is a hole in a spatial index one
/// unit in the last place wide — found by a player, not by a test.
#[test]
fn the_boundary_is_inside() {
    assert!(unit().contains(globalpoint(1, 1, 1)));
    assert!(unit().contains(globalpoint(-1, -1, -1)));
    assert!(!unit().contains(globalpoint(1, 1, 2)));
}

/// The empty box is the identity for merging, so folding nothing gives nothing
/// rather than a degenerate box at the world's centre.
#[test]
fn empty_is_the_identity() {
    assert!(Aabb::EMPTY.is_empty());
    assert_eq!(Aabb::EMPTY.merge(unit()), unit());
    assert_eq!(unit().merge(Aabb::EMPTY), unit());
    assert_eq!(Aabb::from_points([]), Aabb::EMPTY);
    assert!(!Aabb::EMPTY.contains(GlobalPoint::ZERO));
}

#[test]
fn a_box_grows_to_hold_what_it_is_given() {
    let grown = Aabb::from_points([globalpoint(1, 2, 3), globalpoint(-4, 0, 5)]);
    assert_eq!(grown.min, globalpoint(-4, 0, 3));
    assert_eq!(grown.max, globalpoint(1, 2, 5));
}

/// A single point is a box that holds it and nothing else.
#[test]
fn one_point_is_a_degenerate_box() {
    let dot = Aabb::from_points([globalpoint(1, 2, 3)]);
    assert!(!dot.is_empty());
    assert!(dot.contains(globalpoint(1, 2, 3)));
    assert_eq!(dot.half_extent(), GlobalPoint::ZERO);
}

#[test]
fn boxes_that_touch_intersect() {
    let left = Aabb::new(globalpoint(-2, -1, -1), globalpoint(0, 1, 1));
    let right = Aabb::new(GlobalPoint::ZERO, globalpoint(2, 1, 1));
    assert!(left.intersects(&right));
    assert!(!left.intersects(&Aabb::new(globalpoint(1, -1, -1), globalpoint(2, 1, 1))));
}

/// A box and the box `around` its own centre and half extent are the same box.
#[test]
fn a_box_reconstructs_itself() {
    let box_ = Aabb::new(globalpoint(-4, 0, 3), globalpoint(2, 2, 5));
    assert_eq!(Aabb::around(box_.centre(), box_.half_extent()), box_);
}

#[test]
fn a_ray_hits_a_box() {
    let hit = Ray::new(globalpoint(0, -10, 0), Direction::Y)
        .cast_against(&unit())
        .expect("it points at it");
    assert_eq!(hit.distance, I24F8::from_f64(9.0));
    assert_eq!(hit.point, globalpoint(0, -1, 0));
    assert_eq!(hit.normal, -Direction::Y);
}

/// The axis-aligned miss: a ray parallel to two slabs and outside the third.
/// This is the case the divide-by-zero branch exists for, and dividing anyway
/// is what makes it a hit at infinity instead.
#[test]
fn a_parallel_ray_outside_the_slab_misses() {
    assert!(
        Ray::new(globalpoint(5, -10, 0), Direction::Y)
            .cast_against(&unit())
            .is_none()
    );
}

/// And one parallel to two slabs but inside the third arrives, because that
/// axis constrains nothing.
#[test]
fn a_parallel_ray_inside_the_slab_arrives() {
    let hit = Ray::new(globalpoint(0, -10, 0), Direction::Y).cast_against(&unit());
    assert!(hit.is_some());
}

/// A ray starting inside leaves through the far face rather than answering
/// nothing or a negative.
#[test]
fn a_ray_inside_a_box_hits_the_exit() {
    let hit = Ray::new(GlobalPoint::ZERO, Direction::Y)
        .cast_against(&unit())
        .expect("it is surrounded");
    assert_eq!(hit.distance, I24F8::ONE);
    assert_eq!(hit.point, globalpoint(0, 1, 0));
}

/// A ray pointing away from a box in front of it misses.
#[test]
fn a_ray_pointing_away_misses() {
    assert!(
        Ray::new(globalpoint(0, -10, 0), -Direction::Y)
            .cast_against(&unit())
            .is_none()
    );
}

/// A ray that clips a corner along a diagonal still finds it, which is the case
/// where all three slabs constrain the answer at once.
#[test]
fn a_diagonal_ray_finds_the_corner() {
    let towards = globalpoint(1, 1, 1)
        .normalize()
        .expect("it is not the origin");
    let hit = Ray::new(globalpoint(-5, -5, -5), towards).cast_against(&unit());
    assert!(hit.is_some());
}

/// Nothing can be hit inside an empty box.
#[test]
fn an_empty_box_cannot_be_hit() {
    assert!(
        Ray::new(globalpoint(0, -10, 0), Direction::Y)
            .cast_against(&Aabb::EMPTY)
            .is_none()
    );
}

/// A ray with no direction is not a ray, and cannot hit anything.
///
/// `Direction::ZERO` is representable — every point type in `corvid_vector` has
/// a `ZERO`, and a normalization that failed has to answer *something* — so a
/// cast has to decide what it means. It means a miss: a ray that goes nowhere
/// arrives nowhere.
///
/// The slab test is the one that gets this wrong if nobody says so. With every
/// slope zero, no axis divides and no axis constrains, so the entry and the
/// exit keep the sentinels they were seeded with — and the exit sentinel is
/// positive, which reads as a hit at the far end of the world.
#[test]
fn a_ray_with_no_direction_hits_nothing() {
    let nowhere = Ray::new(GlobalPoint::ZERO, Direction::ZERO);
    assert!(nowhere.cast_against(&unit()).is_none());
    assert!(
        Ray::new(globalpoint(0, -10, 0), Direction::ZERO)
            .cast_against(&unit())
            .is_none()
    );
}
