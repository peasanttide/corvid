//! Projection onto a direction, and the `i64` bound the whole thing rests on.
//!
//! `src/point/project.rs` argues that three Q39 products summed fit an `i64`
//! because a direction is a unit vector. That is an algebraic claim about a
//! bound, and the interesting thing about it is that the bound is *tight* --
//! reached exactly, by a direction down the diagonal against the opposite
//! corner of the world. A claim that is 13% from overflowing is worth checking
//! against the arithmetic rather than against the reasoning that produced it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::cast_possible_wrap,
    reason = "the generator answers a bit pattern, and a bit pattern is what from_bits takes"
)]

mod common;

use common::Rng;
use corvid_fixed::{I24F8, Signed32};
use corvid_vector::{Direction, GlobalPoint, globalpoint};

/// The dot of the bit patterns, at full width, so the test is not using the
/// arithmetic it is checking.
fn wide_dot(offset: GlobalPoint, direction: Direction) -> i128 {
    let offsets = offset.to_array();
    let components = direction.to_array();
    (0..3)
        .map(|axis| {
            i128::from(offsets[axis].to_bits())
                * i128::from(components[axis].canonicalize().to_bits())
        })
        .sum()
}

/// The corner of the world that maximises the dot against `direction`: every
/// component at the far edge of its range, signed to agree.
fn worst_corner(direction: Direction) -> GlobalPoint {
    GlobalPoint::from_array(direction.to_array().map(|component| {
        if component < Signed32::ZERO {
            I24F8::MIN
        } else {
            I24F8::MAX
        }
    }))
}

#[test]
fn the_widest_projection_in_the_world_still_fits_an_i64() {
    // The diagonal is the worst case the module names, so check it by hand
    // rather than hope a search wanders into it.
    let diagonal = globalpoint(1, 1, 1).normalize().expect("non-zero");
    let mut worst = wide_dot(worst_corner(diagonal), diagonal).abs();

    // And then look for anything worse, in case the diagonal is not in fact
    // where the maximum sits.
    let mut rng = Rng::new(0xB01D_5EED);
    for _ in 0..20_000 {
        let Some(direction) = globalpoint(
            I24F8::from_bits(rng.next_u32() as i32),
            I24F8::from_bits(rng.next_u32() as i32),
            I24F8::from_bits(rng.next_u32() as i32),
        )
        .normalize() else {
            continue;
        };
        worst = worst.max(wide_dot(worst_corner(direction), direction).abs());
    }

    let ceiling = i128::from(i64::MAX);
    assert!(
        worst <= ceiling,
        "a projection reached {worst}, which an i64 does not hold"
    );

    // And the bound is tight rather than generous, so the headroom the module
    // claims is the headroom there is: no more of it can be spent.
    assert!(
        worst * 10 > ceiling * 8,
        "the widest projection is only {worst} of {ceiling}, so the bound is looser than documented",
    );
}

#[test]
fn projecting_onto_an_axis_is_the_component_itself() {
    let point = globalpoint(3, -4, 12);
    assert_eq!(point.project(Direction::X), I24F8::from(3));
    assert_eq!(point.project(Direction::Y), I24F8::from(-4));
    assert_eq!(point.project(Direction::Z), I24F8::from(12));

    // Exactly, at every axis and both signs -- which is what dividing by the
    // unit rather than shifting by 31 buys, and the reason it is worth the
    // division.
    assert_eq!(point.project(-Direction::X), I24F8::from(-3));
    assert_eq!(point.project(-Direction::Y), I24F8::from(4));
    assert_eq!(point.project(-Direction::Z), I24F8::from(-12));
}

#[test]
fn a_projection_matches_an_f64_reference() {
    let mut rng = Rng::new(0x9E0A_DDED);
    let mut worst = 0.0_f64;
    for _ in 0..20_000 {
        let point = common::random_global_point(&mut rng, 10_000.0);
        let Some(direction) = common::random_global_point(&mut rng, 1_000.0).normalize() else {
            continue;
        };
        let components = direction.to_array();
        let expected: f64 = point
            .to_array()
            .iter()
            .zip(components)
            .map(|(along, component)| along.to_f64() * component.to_f64())
            .sum();
        worst = worst.max((point.project(direction).to_f64() - expected).abs());
    }
    // One step of `I24F8` is 3.9 mm and the rounding is to nearest, so half a
    // step is the most a correct implementation can be out -- plus whatever the
    // direction itself lost when it was normalized, which is what the rest of
    // the tolerance is for.
    assert!(worst <= 0.01, "worst projection error {worst} metres");
}

#[test]
fn align_is_one_for_itself_and_minus_one_for_its_opposite() {
    assert_eq!(Direction::Y.align(Direction::Y), Signed32::MAX);
    assert_eq!(Direction::Y.align(-Direction::Y), Signed32::MIN);
    assert_eq!(Direction::Y.align(Direction::X), Signed32::ZERO);
    assert_eq!(Direction::Y.align(Direction::Z), Signed32::ZERO);
}

#[test]
fn along_walks_the_distance_it_is_given() {
    assert_eq!(Direction::Y.along(I24F8::from(4)), globalpoint(0, 4, 0));
    assert_eq!(
        (-Direction::X).along(I24F8::from_f64(2.5)),
        globalpoint(I24F8::from_f64(-2.5), I24F8::ZERO, I24F8::ZERO),
    );

    // And it is the inverse of projecting back onto the same direction, all the
    // way out to where an `I24F8` runs out of range.
    let far = I24F8::from(8000);
    assert_eq!(Direction::Z.along(far).project(Direction::Z), far);
}
