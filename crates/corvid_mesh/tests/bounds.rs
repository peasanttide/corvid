//! What a mesh's box says, in metres.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::float_cmp,
    reason = "every number compared here is an exact multiple of a quarter and therefore exactly representable in both the fixed-point type it came from and the f64 it is read as; a tolerance would hide the rounding this conversion is supposed to be free of"
)]

use corvid_fixed::I16F16;

use corvid_mesh::{Mesh, Vertex, cube, cylinder, quad};
use corvid_vector::OctDirection;
#[test]
fn a_cube_bounds_itself() {
    // The one conversion this method is: a position component of `FULL` is the
    // mesh's whole scale, so a cube built half a metre from centre to face is
    // exactly a metre across in the box. Exact rather than approximate, because
    // a half is representable in both the `I16F16` the scale is and the `I24F8`
    // the box is.
    let bounds = cube(I16F16::from_f64(0.5)).bounds();
    let metres =
        |corner: corvid_vector::GlobalPoint| corner.to_array().map(corvid_fixed::I24F8::to_f64);
    assert_eq!(metres(bounds.min), [-0.5; 3]);
    assert_eq!(metres(bounds.max), [0.5; 3]);
}

#[test]
fn the_scale_is_what_turns_a_component_into_metres() {
    // The same twenty-four vertices at two scales, and the box is the second
    // scale's. A `bounds` that ignored `scale` would answer the same twice.
    let small = cube(I16F16::ONE).bounds();
    let large = cube(I16F16::from_f64(4.0)).bounds();
    assert_eq!(small.max.x().to_f64(), 1.0);
    assert_eq!(large.max.x().to_f64(), 4.0);
}

#[test]
fn a_flat_mesh_has_a_flat_box() {
    // A quad has no thickness, and the box has to say so rather than rounding
    // it into one: `min` and `max` agree on `z` exactly.
    let bounds = quad(I16F16::ONE).bounds();
    assert!(!bounds.is_empty());
    assert_eq!(bounds.min.z(), bounds.max.z());
    assert_eq!(bounds.min.z().to_f64(), 0.0);
}

#[test]
fn a_mesh_with_two_measurements_bounds_both() {
    // A cylinder's scale is the larger of its radius and its half height, so
    // the shorter axis is a fraction of a full component -- and the box is where
    // that fraction has to come back out as metres.
    let bounds = cylinder(I16F16::from_f64(0.25), I16F16::ONE, 8).bounds();
    assert_eq!(bounds.max.z().to_f64(), 1.0);
    assert_eq!(bounds.max.x().to_f64(), 0.25);
}

#[test]
fn an_empty_mesh_has_an_empty_bound() {
    // Rather than a point at the origin, which is `Aabb::EMPTY`'s whole reason
    // for being inverted: "nothing to draw" and "one degenerate thing at the
    // world's centre" are different answers, and only the second is a culling
    // miss nobody notices.
    let nothing = Mesh::new(Vec::new(), Vec::new(), I16F16::ONE);
    assert!(nothing.bounds().is_empty());
}

#[test]
fn triangles_and_emptiness_are_about_the_indices() {
    // Indices are what a draw call reads, so a mesh carrying vertices no
    // triangle names draws nothing and says so.
    let orphaned = Mesh::new(
        vec![Vertex::new([0, 0, 0], OctDirection::UP)],
        Vec::new(),
        I16F16::ONE,
    );
    assert!(orphaned.is_empty());
    assert_eq!(orphaned.triangles(), 0);
    assert!(
        !orphaned.bounds().is_empty(),
        "the vertex is still somewhere"
    );

    assert_eq!(cube(I16F16::ONE).triangles(), 12);
}
