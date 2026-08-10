//! Squared distances, cross directions, and the determinants a mesh pick is.
//!
//! Two of these are checking a *bound* rather than a behaviour, which is the
//! point of them. Everything here is claimed to fit a word, and the claims are
//! tight enough -- a quarter to a third of headroom -- that the arithmetic is
//! worth asking rather than the algebra.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::cast_precision_loss,
    reason = "the f64 reference is a reference; every comparison against it has a tolerance or is a sign"
)]

mod common;

use common::Rng;
use corvid_fixed::{Factor32, I24F8, I48F16};
use corvid_vector::{Direction, GlobalPoint, Volume, globalpoint};

/// A point at the far edge of the range on one axis.
fn edge(metres: f64) -> GlobalPoint {
    globalpoint(I24F8::from_f64(metres), I24F8::ZERO, I24F8::ZERO)
}

/// The corner that maximises a length against `direction`: every component at
/// the far edge of its range, signed to agree.
fn worst_corner(direction: Direction) -> GlobalPoint {
    GlobalPoint::from_array(direction.to_array().map(|component| {
        if component < corvid_fixed::Signed32::ZERO {
            I24F8::MIN
        } else {
            I24F8::MAX
        }
    }))
}

#[test]
fn a_squared_distance_saturates_above_every_radius_and_below_none() {
    // The comparison a sphere makes is against a squared radius, and a radius
    // is an `I24F8` -- so saturation has to land above the largest of those or
    // the comparison stops being an answer.
    assert!(I24F8::MAX.squared() < I48F16::MAX);
    assert_eq!(edge(8e6).distance_squared(edge(-8e6)), I48F16::MAX);

    // And below saturation it is exact.
    assert_eq!(
        globalpoint(3, 4, 0).distance_squared(GlobalPoint::ZERO),
        I48F16::from(25),
    );
    assert_eq!(
        GlobalPoint::ZERO.distance_squared(globalpoint(0, -12, 0)),
        I48F16::from(144),
    );

    // Symmetric, and zero at a point against itself.
    let here = globalpoint(1, -2, 3);
    assert_eq!(here.distance_squared(here), I48F16::ZERO);
    assert_eq!(
        here.distance_squared(GlobalPoint::ZERO),
        GlobalPoint::ZERO.distance_squared(here),
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
        let along = point.project(direction).squared().to_f64();
        let across = point.rejection_squared(direction).to_f64();
        let total = point.distance_squared(GlobalPoint::ZERO).to_f64();

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
    let offset = globalpoint(0, 4, 0);
    assert_eq!(offset.rejection_squared(Direction::Y), I48F16::ZERO);
    assert_eq!(offset.rejection_squared(-Direction::Y), I48F16::ZERO);
    assert_eq!(offset.rejection_squared(Direction::X), I48F16::from(16));
    assert_eq!(offset.rejection_squared(Direction::Z), I48F16::from(16));
}

#[test]
fn a_rejection_at_the_corner_of_the_range_still_fits_a_word() {
    // The cross product behind it is three two-by-two minors, each bounded by
    // `sqrt(2) * 2^31 * 2^31` rather than the `2^63` two maxed products
    // suggest. A third of headroom, and worth asking for rather than deriving.
    let mut rng = Rng::new(0x_C0FF_EE01);
    let mut worst = 0.0_f64;
    for _ in 0..50_000 {
        let Some(direction) = common::random_global_point(&mut rng, 1_000.0).normalize() else {
            continue;
        };
        let corner = worst_corner(direction);
        // Perpendicular to the direction is where the rejection is largest, so
        // take the corner against a direction it does not point along.
        let across = Direction::from_ratio([
            i64::from(direction.to_array()[1].to_bits()),
            -i64::from(direction.to_array()[0].to_bits()),
            0,
        ]);
        let Some(across) = across else { continue };
        worst = worst.max(corner.rejection_squared(across).to_f64());
        worst = worst.max(corner.rejection_squared(direction).to_f64());
    }
    // Saturation is legitimate at the very corner; what would not be is a
    // negative, which is what an overflow here would answer.
    assert!(worst >= 0.0);
    assert!(worst <= I48F16::MAX.to_f64());
}

#[test]
fn a_cross_of_two_range_spanning_edges_still_names_the_right_normal() {
    // Two 8 000 km edges cross to `2^62`, which is a word and nothing
    // narrower. Dividing that back into a component's range before normalizing
    // answers a direction the face does not have.
    let first = globalpoint(I24F8::from_f64(8e6), I24F8::ZERO, I24F8::from_f64(2e6));
    let second = globalpoint(I24F8::ZERO, I24F8::from_f64(8e6), I24F8::ZERO);

    let wide = first.cross_direction(second).expect("not parallel");
    let narrowed = first.cross(second).normalize().expect("not parallel");

    // `(-1.6e13, 0, 6.4e13)` normalizes to `(-0.2425, 0, 0.9701)`.
    assert_eq!(
        wide,
        Direction::from_ratio([-16_000_000_000_000, 0, 64_000_000_000_000]).expect("not zero"),
    );
    assert!(
        (wide.to_array()[0].to_f64() + 0.242_535).abs() < 1e-5,
        "{wide:?}"
    );
    // The narrowed one saturates on two axes and comes back 31 degrees out.
    assert_ne!(narrowed, wide);

    // Parallel edges span no plane and name no normal.
    assert_eq!(first.cross_direction(first), None);
    assert_eq!(GlobalPoint::ZERO.cross_direction(second), None);
}

#[test]
fn a_volume_is_signed_by_the_winding_and_zero_in_a_plane() {
    let x = globalpoint(2, 0, 0);
    let y = globalpoint(0, 3, 0);

    // Two offsets and a direction that lie in a plane span nothing.
    assert!(x.volume(Direction::X, y).is_zero());
    assert!(x.volume(Direction::Y, y).is_zero());

    // Out of the plane, and reversing the two offsets reverses the sign.
    let right_handed = x.volume(Direction::Z, y);
    assert!(!right_handed.is_zero());
    assert_ne!(right_handed, y.volume(Direction::Z, x));

    // `is_outside` is the barycentric comparison: inside is between zero and
    // the bound, at either sign of the bound.
    assert!(!right_handed.is_outside(right_handed));
    assert!(y.volume(Direction::Z, x).is_outside(right_handed));
    assert!(!right_handed.is_outside(right_handed.add(right_handed)));
}

/// The triple product in `f64`, at the same arbitrary scale `volume` uses, so
/// the test is not checking the arithmetic against itself.
fn reference_volume(a: GlobalPoint, direction: Direction, b: GlobalPoint) -> f64 {
    let a = a.to_array().map(|c| f64::from(c.to_bits()));
    let d = direction.to_array().map(|c| f64::from(c.to_bits()));
    let b = b.to_array().map(|c| f64::from(c.to_bits()));
    let cross = [
        d[1] * b[2] - d[2] * b[1],
        d[2] * b[0] - d[0] * b[2],
        d[0] * b[1] - d[1] * b[0],
    ];
    (a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2]) / (2.0 * f64::from(i32::MAX))
}

/// One of the eight corners of the range, by the three bits of `which`.
fn corner(which: u32) -> GlobalPoint {
    let pick = |bit: u32| {
        if which & (1 << bit) == 0 {
            I24F8::MIN
        } else {
            I24F8::MAX
        }
    };
    GlobalPoint::new(pick(0), pick(1), pick(2))
}

#[test]
fn a_volume_at_the_corner_of_the_range_still_fits_a_word() {
    // `volume` halves the cross on the way past, which takes the bound from
    // `3 * 2^62` -- half again too much -- to `2^62.6`, a quarter inside a
    // word. A product that wrapped instead of fitting comes back with the
    // opposite sign, so the check is that the winding never disagrees with an
    // `f64` reference, over every pair of corners the range has.
    let mut rng = Rng::new(0xD00D_1E55);
    let mut largest = 0.0_f64;
    let mut checked = 0u32;
    for _ in 0..2_000 {
        let Some(direction) = common::random_global_point(&mut rng, 1_000.0).normalize() else {
            continue;
        };
        for first in 0..8 {
            for second in 0..8 {
                let (a, b) = (corner(first), corner(second));
                let reference = reference_volume(a, direction, b);
                largest = largest.max(reference.abs());
                // Far enough from zero that rounding cannot decide the sign.
                if reference.abs() > 1e15 {
                    checked += 1;
                    assert_eq!(
                        a.volume(direction, b) > Volume::ZERO,
                        reference > 0.0,
                        "a volume of {reference:e} came back with the wrong sign",
                    );
                }
            }
        }
    }

    assert!(
        checked > 10_000,
        "the sweep only reached {checked} signed cases"
    );
    let ceiling = i64::MAX as f64;
    assert!(
        largest < ceiling,
        "a volume of {largest:e} does not fit a word"
    );
    // And the search really does approach the bound, so this is not passing
    // because it never went anywhere near it.
    assert!(
        largest * 2.0 > ceiling,
        "the widest volume found was {largest:e} of {ceiling:e}",
    );
}

#[test]
fn a_ratio_is_the_weight_it_was_asked_for() {
    let x = globalpoint(2, 0, 0);
    let y = globalpoint(0, 3, 0);
    let whole = x.volume(Direction::Z, y);

    assert_eq!(whole.ratio(whole), Factor32::MAX);
    assert_eq!(Volume::ZERO.ratio(whole), Factor32::ZERO);

    // Half the offset is half the volume, to within the factor's own last bits.
    let half = globalpoint(1, 0, 0).volume(Direction::Z, y);
    let weight = half.ratio(whole).to_f64();
    assert!(
        (weight - 0.5).abs() < 1e-6,
        "half a volume weighed {weight}"
    );
}
