#![doc = include_str!("../README.md")]
#![no_std]

/// The library, whole. A game doing its own linear algebra needs names this
/// crate has no reason to enumerate -- decompositions, slices, the iterator
/// adaptors -- and reaching them through here rather than through a `nalgebra`
/// line in the game's own manifest is what keeps one nalgebra in the graph.
///
/// The second copy is the failure this prevents. A game that writes down a
/// version this workspace is not on gets its own nalgebra, whose `Vector3<f32>`
/// is a different type from [`Vec3`] and will not pass for one.
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

/// A 3x3 matrix: a rotation or a normal transform, without the translation.
///
/// Column-major like [`Mat4`], and further from what a shader reads than
/// [`Mat4`] is. This is nine floats packed into thirty-six bytes; a WGSL
/// `mat3x3<f32>` pads every column out to sixteen and occupies forty-eight.
/// Where [`Mat4`] parts company with the shader only over alignment, this parts
/// company over the bytes themselves -- the order is right and the stride is not
/// -- so a normal matrix bound to a buffer goes across as a [`Mat4`] or as three
/// [`Vec4`]s. This type is the CPU side of that.
pub type Mat3 = nalgebra::Matrix3<f32>;

/// A 4x4 matrix, column-major -- the order a WGSL `mat4x4` reads.
///
/// **The order, which is not the whole layout.** The sixty-four bytes are in
/// the order a `mat4x4<f32>` wants, and that is what makes handing one to a
/// buffer a copy rather than a transpose. It is not a promise that this type
/// *is* the shader's type: its Rust alignment is four, where WGSL gives a
/// matrix sixteen. At an offset that is already sixteen-aligned -- the start of
/// a buffer, a binding of its own -- nothing can observe the difference, and
/// that is the common case. Inside a `#[repr(C)]` struct that is cast to bytes
/// it can be observed: Rust will place the field on four and the shader will
/// read it on sixteen, and they part company at the first field that does not
/// happen to land on both. A struct crossing to a shader owes its own padding.
/// The same gap applies to [`Vec3`] and [`Vec4`], which WGSL also aligns to
/// sixteen and this crate aligns to four.
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
/// // Indexing is `(row, column)`, which would read the same under either
/// // convention and so proves nothing on its own.
/// assert_eq!(MOVED[(0, 3)], 5.0);
///
/// // The storage is where the convention shows: the translation is the last
/// // four floats of the sixty-four bytes, which is exactly where a shader
/// // reading a column-major `mat4x4<f32>` looks for it.
/// assert_eq!(&MOVED.as_slice()[12..], [5.0, 0.0, 0.0, 1.0]);
/// ```
pub type Mat4 = nalgebra::Matrix4<f32>;

/// The 4x4 identity, as a `const`.
///
/// [`Mat4::identity`](nalgebra::Matrix4::identity) is a function rather than a
/// constant, so a caller wanting one at compile time -- a default camera, a
/// model matrix a game overwrites per instance -- would have to build it at
/// runtime. This is that value.
pub const IDENTITY: Mat4 = Mat4::new(
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
);
