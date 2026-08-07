//! What a shape looks like once it has left the process.

// A build without `serde` compiles nothing here rather than half of it.
#![cfg(feature = "serde")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic_in_result_fn,
    reason = "a failed assertion in a test is a failed test, which is what a test is for; the `Result` is how the encoding's own errors reach the runner"
)]

use corvid_fixed::I24F8;
use corvid_hash::digest;
use corvid_shape::{Aabb, Plane, Ray, Sphere, Triangle};
use corvid_vector::{Direction, GlobalPoint, globalpoint};

/// Every shape survives the workspace's encoding unchanged.
#[test]
fn every_shape_round_trips() -> Result<(), corvid_wire::Error> {
    let ray = Ray::new(globalpoint(1, 2, 3), Direction::Y);
    assert_eq!(
        corvid_wire::decode::<Ray>(&corvid_wire::encode(&ray)?)?,
        ray
    );

    let ball = Sphere::new(globalpoint(0, 10, 0), I24F8::from_f64(2.0));
    assert_eq!(
        corvid_wire::decode::<Sphere>(&corvid_wire::encode(&ball)?)?,
        ball
    );

    let bounds = Aabb::new(globalpoint(-1, -1, -1), globalpoint(1, 1, 1));
    assert_eq!(
        corvid_wire::decode::<Aabb>(&corvid_wire::encode(&bounds)?)?,
        bounds
    );

    let ground = Plane::through(GlobalPoint::ZERO, Direction::Z);
    assert_eq!(
        corvid_wire::decode::<Plane>(&corvid_wire::encode(&ground)?)?,
        ground
    );

    let face = Triangle::new(
        globalpoint(-1, 5, -1),
        globalpoint(1, 5, -1),
        globalpoint(0, 5, 1),
    );
    assert_eq!(
        corvid_wire::decode::<Triangle>(&corvid_wire::encode(&face)?)?,
        face
    );

    let hit = Ray::new(GlobalPoint::ZERO, Direction::Y)
        .cast_against(&ball)
        .expect("it points straight at it");
    assert_eq!(
        corvid_wire::decode::<corvid_shape::Hit>(&corvid_wire::encode(&hit)?)?,
        hit
    );

    Ok(())
}

/// The same shape digests the same, and a neighbouring one does not.
#[test]
fn a_shape_digests() {
    let ball = Sphere::new(globalpoint(0, 10, 0), I24F8::from_f64(2.0));
    assert_eq!(
        digest(&ball),
        digest(&Sphere::new(globalpoint(0, 10, 0), I24F8::from_f64(2.0)))
    );
    // One step in the last place, which at `I24F8`'s 3.9 mm is 2.0039 m. A
    // difference finer than that is not a different sphere, so asserting on one
    // would be asserting about the digest rather than about the type.
    assert_ne!(
        digest(&ball),
        digest(&Sphere::new(
            globalpoint(0, 10, 0),
            I24F8::from_bits(I24F8::from_f64(2.0).to_bits() + 1)
        ))
    );
}

/// A box's two corners are absorbed in declaration order rather than as an
/// unordered pair, so a box and the inverted box that names the same two points
/// differ.
#[test]
fn the_corners_are_ordered() {
    let low = globalpoint(-1, -1, -1);
    let high = globalpoint(1, 1, 1);
    assert_ne!(digest(&Aabb::new(low, high)), digest(&Aabb::new(high, low)));
}

/// A triangle's winding is in its digest, because it is in its normal.
#[test]
fn the_winding_is_hashed() {
    let a = globalpoint(-1, 5, -1);
    let b = globalpoint(1, 5, -1);
    let c = globalpoint(0, 5, 1);
    assert_ne!(
        digest(&Triangle::new(a, b, c)),
        digest(&Triangle::new(a, c, b))
    );
}
