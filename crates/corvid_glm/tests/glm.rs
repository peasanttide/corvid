//! The aliases, their storage order, and the one `const`.
//!
//! There is no arithmetic in this crate to test — nalgebra's own suite covers
//! that, and duplicating it here would only assert that nalgebra is nalgebra.
//! What is worth pinning is what this crate *claims*: that a matrix is stored
//! down its columns, which is the whole reason the README can promise no
//! transpose on the way to the device, and that `IDENTITY` is still the matrix
//! `Matrix4::identity` builds at runtime.

#![allow(
    clippy::float_cmp,
    reason = "every value here is a small integer, exactly representable in f32, and the point of the assertion is which slot it landed in rather than how close it came"
)]

use core::mem::{align_of, size_of};

use corvid_glm::{IDENTITY, Mat3, Mat4, Vec2, Vec3, Vec4};

/// A matrix whose sixteen entries are all distinct, written across the way
/// `Matrix4::new` takes them. A permutation this shape is the only kind of
/// input that can tell a column-major dump from a row-major one — an identity
/// or a symmetric matrix reads the same either way, which is how a transpose
/// survives a test suite.
const COUNTING: Mat4 = Mat4::new(
    0.0, 1.0, 2.0, 3.0, //
    4.0, 5.0, 6.0, 7.0, //
    8.0, 9.0, 10.0, 11.0, //
    12.0, 13.0, 14.0, 15.0,
);

/// A translation of `(5, 6, 7)`, for the tests that care where the translation
/// physically sits rather than what it does.
const MOVED: Mat4 = Mat4::new(
    1.0, 0.0, 0.0, 5.0, //
    0.0, 1.0, 0.0, 6.0, //
    0.0, 0.0, 1.0, 7.0, //
    0.0, 0.0, 0.0, 1.0,
);

#[test]
fn every_alias_is_its_components_and_nothing_else() {
    // Every alignment here is four, and every one of these types is aligned to
    // sixteen by WGSL. That is not a defect being pinned in place: the payload
    // is what this crate promises, and a payload written at an offset that is
    // already sixteen-aligned is read correctly whatever the Rust type's own
    // alignment says. What the four rules out is casting a `#[repr(C)]` struct
    // with one of these in the middle and expecting the shader to agree about
    // where the field starts. The aliases are asserted rather than described
    // because that is the assumption a caller doing the cast is making.
    assert_eq!((size_of::<Vec2>(), align_of::<Vec2>()), (8, 4));
    assert_eq!((size_of::<Vec3>(), align_of::<Vec3>()), (12, 4));
    assert_eq!((size_of::<Vec4>(), align_of::<Vec4>()), (16, 4));
    assert_eq!((size_of::<Mat4>(), align_of::<Mat4>()), (64, 4));

    // Nine floats packed, not the forty-eight bytes a WGSL `mat3x3<f32>`
    // occupies once each column is padded out to sixteen. `Mat3` is the CPU
    // side of a normal transform; the shader-facing form is a `Mat4` or three
    // `Vec4`s, and the difference is documented on the alias for that reason.
    assert_eq!((size_of::<Mat3>(), align_of::<Mat3>()), (36, 4));
}

#[test]
fn a_matrix_is_stored_down_its_columns() {
    // `as_slice` is the flat storage — what a `memcpy` into a mapped buffer
    // would copy. Written across, it has to come back out down.
    assert_eq!(
        COUNTING.as_slice(),
        [
            0.0, 4.0, 8.0, 12.0, //
            1.0, 5.0, 9.0, 13.0, //
            2.0, 6.0, 10.0, 14.0, //
            3.0, 7.0, 11.0, 15.0,
        ]
    );

    // The same fact stated as the relation between the two accessors: the
    // index operator takes `(row, column)`, and entry `(r, c)` lives at flat
    // offset `c * 4 + r`. Row-major storage would put it at `r * 4 + c`, and
    // because no two entries of `COUNTING` are equal, that would fail here.
    for row in 0..4 {
        for column in 0..4 {
            assert_eq!(
                COUNTING[(row, column)],
                COUNTING.as_slice()[column * 4 + row]
            );
        }
    }
}

#[test]
fn the_translation_is_the_last_column_and_the_last_four_floats() {
    assert_eq!(
        (MOVED[(0, 3)], MOVED[(1, 3)], MOVED[(2, 3)]),
        (5.0, 6.0, 7.0)
    );

    // Offset 12 of 16, which is byte 48 of 64: where a shader reading a
    // column-major `mat4x4<f32>` looks for it.
    assert_eq!(&MOVED.as_slice()[12..], [5.0, 6.0, 7.0, 1.0]);

    // And it is a translation, not merely shaped like one.
    let origin = Vec4::new(0.0, 0.0, 0.0, 1.0);
    assert_eq!(MOVED * origin, Vec4::new(5.0, 6.0, 7.0, 1.0));

    // A `Vec3` homogenizes with a zero fourth component, so a direction comes
    // through a translation unchanged. That is the distinction the `Vec3` doc
    // draws between a difference and a position.
    let direction = Vec3::new(1.0, 0.0, 0.0);
    assert_eq!(
        MOVED * direction.to_homogeneous(),
        direction.to_homogeneous()
    );
}

#[test]
fn identity_is_the_matrix_the_function_builds() {
    // The reason `IDENTITY` exists at all: this line is the one
    // `Matrix4::identity()` cannot be written on, being a function.
    const DEFAULT_MODEL: Mat4 = IDENTITY;

    // Which leaves the risk that the hand-written constant and the function
    // drift apart. They have not.
    assert_eq!(DEFAULT_MODEL, Mat4::identity());

    let v = Vec4::new(1.5, -2.0, 3.25, 1.0);
    assert_eq!(IDENTITY * v, v);
    assert_eq!(IDENTITY * MOVED, MOVED);
    assert_eq!(MOVED * IDENTITY, MOVED);
}

#[test]
fn a_vector_can_be_normalized_without_std() {
    // The assertion behind the `libm` in the manifest. The workspace pins
    // nalgebra at `default-features = false`, and without a float backend `f32`
    // has no `SimdComplexField` impl, so `normalize` is not merely wrong here —
    // it does not compile. A square root is the one piece of the standard
    // library the aliases actually need, and this is it reached without std.
    let v = Vec3::new(3.0, 4.0, 0.0);
    assert_eq!(v.norm(), 5.0);

    // Exact: the norm is 5 with no rounding, and dividing 3 and 4 by it is one
    // correctly rounded operation each, landing on the same `f32` the literals
    // do.
    let unit = v.normalize();
    assert_eq!((unit.x, unit.y, unit.z), (0.6, 0.8, 0.0));
}

#[test]
fn the_re_exported_nalgebra_is_the_one_the_aliases_are_made_of() {
    // The compile is the assertion. A downstream that reaches nalgebra through
    // this re-export and one that names its own copy would produce two
    // distinct `Vector3<f32>` types, and this annotation would not typecheck —
    // which is the failure the re-export exists to prevent.
    let v: corvid_glm::nalgebra::Vector3<f32> = Vec3::new(1.0, 2.0, 3.0);
    assert_eq!(v, Vec3::new(1.0, 2.0, 3.0));

    let m: corvid_glm::nalgebra::Matrix4<f32> = IDENTITY;
    assert_eq!(m, Mat4::identity());
}
