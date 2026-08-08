#![doc = include_str!("../README.md")]
#![no_std]

// The library, whole. A game doing its own linear algebra needs names this
// crate has no reason to enumerate — decompositions, slices, the iterator
// adaptors — and re-exporting the crate is what keeps one nalgebra in the
// graph rather than a second one a game added to its own manifest.
pub use nalgebra;

/// A two-component vector: a texture coordinate, a screen position.
pub type Vec2 = nalgebra::Vector2<f32>;

/// A three-component vector: a direction, or a position with the eye already
/// subtracted.
///
/// A world position is **not** one of these. Positions are fixed-point
/// `GlobalPoint`s until the eye has been taken out of them, because an `f32`
/// has twenty-four bits of mantissa and the world is 8388 km across. This is
/// what a difference becomes afterwards.
pub type Vec3 = nalgebra::Vector3<f32>;

/// A four-component vector: a homogeneous position, or a colour.
pub type Vec4 = nalgebra::Vector4<f32>;

/// A 3×3 matrix: a rotation or a normal transform, without the translation.
pub type Mat3 = nalgebra::Matrix3<f32>;

/// A 4×4 matrix, column-major — the order a WGSL `mat4x4` reads.
///
/// ```
/// use corvid_glm::Mat4;
///
/// // `new` takes rows and stores columns, so the translation reads down the
/// // last column and is written across the last argument of each row.
/// const MOVED: Mat4 = Mat4::new(
///     1.0, 0.0, 0.0, 5.0,
///     0.0, 1.0, 0.0, 0.0,
///     0.0, 0.0, 1.0, 0.0,
///     0.0, 0.0, 0.0, 1.0,
/// );
///
/// assert_eq!(MOVED[(0, 3)], 5.0);
/// ```
pub type Mat4 = nalgebra::Matrix4<f32>;

/// The 4×4 identity, as a `const`.
///
/// [`Mat4::identity`](nalgebra::Matrix4::identity) is a function rather than a
/// constant, so a caller wanting one at compile time — a default camera, a
/// model matrix a game overwrites per instance — would have to build it at
/// runtime. This is that value.
pub const IDENTITY: Mat4 = Mat4::new(
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
);
