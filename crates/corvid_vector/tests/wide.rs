//! The offset between two points too far apart to subtract.
//!
//! Every test here has the same shape: put the two points further apart than a
//! component reaches, and check that the answer is the geometry rather than the
//! clamp. That is the whole reason the type exists, and each of these pins a
//! bug that a saturating subtraction actually produced.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::Rng;
use corvid_fixed::{I24F8, I48F16};
use corvid_vector::{Direction, GlobalPoint, Volume, WideOffset, globalpoint};

/// A point at the far edge of the world on one axis.
fn edge(metres: f64) -> GlobalPoint {
    globalpoint(I24F8::from_f64(metres), I24F8::ZERO, I24F8::ZERO)
}

#[test]
fn a_difference_wider_than_a_component_keeps_its_middle() {
    // 12 000 km across, on a component that stops at 8 388. The saturating
    // subtraction answered 4 194 km for this, and the box's centre came back
    // 1 805 km off the origin.
    let (low, high) = (edge(-6e6), edge(6e6));
    assert_eq!(WideOffset::between(high, low).half(), edge(6e6));
    assert_eq!(
        low + WideOffset::between(high, low).half(),
        GlobalPoint::ZERO
    );

    // The same offset narrowed *is* the clamp, which is the contrast worth
    // pinning: the type is not hiding the range, it is halving before it.
    assert_eq!(
        WideOffset::between(high, low).narrow(),
        globalpoint(I24F8::MAX, I24F8::ZERO, I24F8::ZERO)
    );
}

#[test]
fn a_bearing_across_the_world_is_not_a_diagonal() {
    // Past the range in x and a tenth of that in y. Two saturating subtractions
    // clamp only the first, which turns a shallow bearing into a steep one.
    let from = globalpoint(I24F8::from_f64(-8e6), I24F8::from_f64(-8e5), I24F8::ZERO);
    let to = globalpoint(I24F8::from_f64(8e6), I24F8::from_f64(8e5), I24F8::ZERO);

    let wide = WideOffset::between(to, from)
        .direction()
        .expect("not the same point");
    let narrowed = (to - from).normalize().expect("not the same point");

    // The true bearing is atan(0.1), about 5.7 degrees off the x axis.
    let slope =
        |direction: Direction| direction.to_array()[1].to_f64() / direction.to_array()[0].to_f64();
    assert!(
        (slope(wide) - 0.1).abs() < 1e-6,
        "wide bearing was {}",
        slope(wide)
    );
    assert!(
        slope(narrowed) > 0.19,
        "the saturating difference did not clamp after all"
    );
}

#[test]
fn a_projection_is_the_same_number_the_narrow_one_answers() {
    // The two take different routes -- one division against three, and a
    // remainder carried between them -- and the claim is that they land on the
    // same integer. Worth checking over the whole range rather than at a
    // handful of points.
    let mut rng = Rng::new(0x0FF5_E7A1);
    for _ in 0..50_000 {
        let point = common::random_global_point(&mut rng, 4_000_000.0);
        let Some(direction) = common::random_global_point(&mut rng, 1_000.0).normalize() else {
            continue;
        };
        assert_eq!(
            WideOffset::between(point, GlobalPoint::ZERO).project(direction),
            point.project(direction),
            "the wide and narrow projections disagreed at {point:?} along {direction:?}",
        );
    }
}

#[test]
fn a_projection_across_the_world_is_not_clamped_before_it_is_taken() {
    // 16 000 km along x, projected onto x. The answer does not fit an `I24F8`
    // and clamps -- but onto the *perpendicular* it is exactly zero, which a
    // clamped offset would not have managed.
    let offset = WideOffset::between(edge(8e6), edge(-8e6));
    assert_eq!(offset.project(Direction::X), I24F8::MAX);
    assert_eq!(offset.project(Direction::Y), I24F8::ZERO);
    assert_eq!(offset.project(-Direction::X), I24F8::MIN);
}

#[test]
fn a_squared_length_saturates_above_every_radius_and_below_none() {
    // The comparison a sphere makes is against a squared radius, and a radius
    // is an `I24F8` -- so saturation has to land above the largest of those or
    // the comparison stops being an answer.
    let across = WideOffset::between(edge(8e6), edge(-8e6));
    assert_eq!(across.length_squared(), I48F16::MAX);
    assert!(I24F8::MAX.squared() < I48F16::MAX);

    // And below saturation it is exact.
    assert_eq!(
        WideOffset::between(globalpoint(3, 4, 0), GlobalPoint::ZERO).length_squared(),
        I48F16::from(25),
    );
}

#[test]
fn a_rejection_is_what_the_projection_leaves() {
    // Pythagoras, at the doubled scale: the squared length is the squared
    // projection plus the squared rejection.
    let mut rng = Rng::new(0x4E1E_C701);
    for _ in 0..20_000 {
        let point = common::random_global_point(&mut rng, 1_000_000.0);
        let Some(direction) = common::random_global_point(&mut rng, 1_000.0).normalize() else {
            continue;
        };
        let offset = WideOffset::between(point, GlobalPoint::ZERO);
        let along = offset.project(direction).squared().to_f64();
        let across = offset.rejection_squared(direction).to_f64();
        let total = offset.length_squared().to_f64();

        // One part in a million of the total, which is the two roundings the
        // squares carry rather than a disagreement about the geometry.
        let apart = (along + across - total).abs();
        assert!(
            apart <= total / 1e6 + 1.0,
            "{along} + {across} is not {total}"
        );
    }
}

#[test]
fn a_rejection_is_zero_along_the_offset_and_everything_across_it() {
    let offset = WideOffset::between(globalpoint(0, 4, 0), GlobalPoint::ZERO);
    assert_eq!(offset.rejection_squared(Direction::Y), I48F16::ZERO);
    assert_eq!(offset.rejection_squared(-Direction::Y), I48F16::ZERO);
    assert_eq!(offset.rejection_squared(Direction::X), I48F16::from(16));
}

#[test]
fn a_cross_of_two_world_spanning_offsets_still_names_the_right_normal() {
    // A triangle in the z = 0 plane, spanning the world. Both edges and the
    // cross product itself leave a component's range, and narrowing any of them
    // first answers a normal that is not perpendicular to the face.
    let a = globalpoint(I24F8::from_f64(-8e6), I24F8::from_f64(-8e6), I24F8::ZERO);
    let b = globalpoint(I24F8::from_f64(8e6), I24F8::from_f64(-8e6), I24F8::ZERO);
    let c = globalpoint(I24F8::from_f64(-8e6), I24F8::from_f64(8e6), I24F8::ZERO);

    let first = WideOffset::between(b, a);
    let second = WideOffset::between(c, a);
    assert_eq!(first.cross_direction(second), Some(Direction::Z));
    assert_eq!(second.cross_direction(first), Some(-Direction::Z));

    // Two parallel edges span no plane and name no normal.
    assert_eq!(first.cross_direction(first), None);
}

#[test]
fn a_volume_is_signed_by_the_winding_and_zero_in_a_plane() {
    let x = WideOffset::between(globalpoint(2, 0, 0), GlobalPoint::ZERO);
    let y = WideOffset::between(globalpoint(0, 3, 0), GlobalPoint::ZERO);
    let z = WideOffset::between(globalpoint(0, 0, 5), GlobalPoint::ZERO);

    // Three offsets that lie in a plane span nothing, whichever two of them
    // are the same one.
    assert_eq!(x.volume(y, x), Volume::ZERO);
    assert!(x.volume(y, x).is_zero());

    // Reversing two of them reverses the sign and nothing else, which is what
    // makes a winding test a sign test.
    let right_handed = x.volume(y, z);
    assert!(!right_handed.is_zero());
    assert_ne!(right_handed, x.volume(z, y));
    assert_eq!(right_handed.add(x.volume(z, y)), Volume::ZERO);

    // `is_outside` is the barycentric comparison: inside is between zero and
    // the bound, at either sign of the bound.
    assert!(!Volume::ZERO.is_outside(right_handed));
    assert!(!right_handed.is_outside(right_handed));
    assert!(x.volume(z, y).is_outside(right_handed));
}

#[test]
fn an_offset_between_a_point_and_itself_is_zero() {
    let point = globalpoint(1, -2, 3);
    let offset = WideOffset::between(point, point);
    assert!(offset.is_zero());
    assert_eq!(offset.direction(), None);
    assert_eq!(offset.narrow(), GlobalPoint::ZERO);
    assert_eq!(offset.half(), GlobalPoint::ZERO);
    assert_eq!(offset.length_squared(), I48F16::ZERO);
}
