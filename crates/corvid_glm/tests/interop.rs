//! The optional integrations.
//!
//! Each feature's tests are gated on that feature, so this file compiles and
//! passes with any subset enabled. Run with `--all-features` to exercise all of
//! it.
//!
//! Neither integration is written in this crate — both are nalgebra's, switched
//! on through a forwarding feature. What is being tested is therefore the
//! wiring rather than the impls: that enabling the feature here really does
//! reach the impl, and that a matrix crossing either boundary keeps the
//! column-major order the rest of the crate promises.

#![allow(
    clippy::float_cmp,
    reason = "every value here is a small integer, exactly representable in f32, and the point of the assertion is which slot it landed in rather than how close it came"
)]

#[cfg(any(feature = "bytemuck", feature = "mint"))]
use corvid_glm::Mat4;

/// A translation of `(5, 6, 7)`. Asymmetric on purpose: an identity crossing a
/// boundary that transposes still arrives as an identity, so it proves nothing.
#[cfg(any(feature = "bytemuck", feature = "mint"))]
const MOVED: Mat4 = Mat4::new(
    1.0, 0.0, 0.0, 5.0, //
    0.0, 1.0, 0.0, 6.0, //
    0.0, 0.0, 1.0, 7.0, //
    0.0, 0.0, 0.0, 1.0,
);

#[cfg(feature = "bytemuck")]
#[test]
fn the_aliases_are_pod() {
    use corvid_glm::{Mat3, Vec2, Vec3, Vec4};

    // The bound is the test: `Pod` is what lets a camera be written into a
    // uniform buffer with no `unsafe` block in this workspace, which forbids
    // `unsafe_code` outright and so could not have written these impls itself.
    const fn assert_pod<T: bytemuck::Pod>() {}
    assert_pod::<Vec2>();
    assert_pod::<Vec3>();
    assert_pod::<Vec4>();
    assert_pod::<Mat3>();
    assert_pod::<Mat4>();

    assert_eq!(<Mat4 as bytemuck::Zeroable>::zeroed(), Mat4::zeros());
}

#[cfg(feature = "bytemuck")]
#[test]
fn a_matrix_reaches_a_buffer_as_sixty_four_column_major_bytes() {
    let bytes = bytemuck::bytes_of(&MOVED);
    assert_eq!(bytes.len(), 64);

    // Spelled as bytes rather than as floats, because bytes are what is copied
    // into the buffer. Native order, not little-endian: `bytes_of` reinterprets
    // the value in place, so a big-endian host should see big-endian floats and
    // an assertion pinned to `to_le_bytes` would be wrong there rather than
    // stricter.
    let mut expected = [0_u8; 64];
    for (slot, value) in [
        1.0_f32, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        5.0, 6.0, 7.0, 1.0,
    ]
    .into_iter()
    .enumerate()
    {
        expected[slot * 4..][..4].copy_from_slice(&value.to_ne_bytes());
    }

    // The translation landing at byte 48 and not at bytes 12, 28 and 44 is the
    // difference between column-major and row-major, and so between a matrix a
    // WGSL `mat4x4<f32>` reads correctly and one it reads transposed.
    assert_eq!(bytes, expected);
}

#[cfg(feature = "mint")]
#[test]
fn the_vectors_cross_to_mint_component_for_component() {
    use corvid_glm::{Vec2, Vec3, Vec4};

    let three: mint::Vector3<f32> = Vec3::new(1.0, 2.0, 3.0).into();
    assert_eq!((three.x, three.y, three.z), (1.0, 2.0, 3.0));
    assert_eq!(Vec3::from(three), Vec3::new(1.0, 2.0, 3.0));

    let two: mint::Vector2<f32> = Vec2::new(1.0, 2.0).into();
    assert_eq!((two.x, two.y), (1.0, 2.0));
    assert_eq!(Vec2::from(two), Vec2::new(1.0, 2.0));

    let four: mint::Vector4<f32> = Vec4::new(1.0, 2.0, 3.0, 4.0).into();
    assert_eq!((four.x, four.y, four.z, four.w), (1.0, 2.0, 3.0, 4.0));
    assert_eq!(Vec4::from(four), Vec4::new(1.0, 2.0, 3.0, 4.0));
}

#[cfg(feature = "mint")]
#[test]
fn a_matrix_crosses_to_mint_as_columns() {
    // mint distinguishes the two conventions in the type name, so this is the
    // one place the crate's column-major choice is written down in something
    // other than prose: `ColumnMatrix4`, and its `w` member is the fourth
    // *column* — the translation — rather than the fourth row.
    let out: mint::ColumnMatrix4<f32> = MOVED.into();
    assert_eq!((out.w.x, out.w.y, out.w.z, out.w.w), (5.0, 6.0, 7.0, 1.0));
    assert_eq!(Mat4::from(out), MOVED);
}
