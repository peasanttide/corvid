//! Fixed-point poses turned into the floating-point matrices a device wants.
//!
//! This module is the boundary. Everything above it — every position, every
//! rotation, every angle a simulation or a camera produces — is fixed-point.
//! Everything below it is `f32`, because that is what a GPU has. Nothing that
//! happens here is hashed, sent or replayed, so the rounding is free.
//!
//! It names no graphics library. That is what lets it live a crate below the
//! device: a headless determinism check builds the same matrices a windowed
//! run does, and a game that writes its own shader still has to answer "which
//! way is up in clip space" — getting that wrong puts a picture on the screen
//! upside down rather than failing.
//!
//! # Camera-relative
//!
//! The eye is subtracted before anything reaches `f32`. A `GlobalFinePoint`
//! reaches 1.4e14 m and an `f32` has twenty-four bits of mantissa, so a
//! position converted directly would quantize to metres at planetary distance
//! and to kilometres past that. Subtracting in `f64` first and converting the
//! *difference* means the precision follows the camera, which is what makes a
//! cube a metre away solid while the planet it is standing on is ten thousand
//! kilometres across.
//!
//! So the pair below is a pair: [`model`] takes the eye's position out and
//! [`view`] takes only the eye's rotation, because the translation has already
//! gone. Using one without the other draws the world at the origin.
//!
//! # Column-major
//!
//! A [`Mat4`] is `corvid_glm`'s, which is nalgebra's, which is column-major —
//! the order a WGSL `mat4x4` reads. There is no transpose on the way to a
//! device and no `columns()` to forget to call. The builders below are written
//! with [`Mat4::new`](corvid_glm::nalgebra::Matrix4::new), which takes its
//! arguments row by row and stores them column by column, so a matrix still
//! *reads* across the page.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "this module is the boundary between the workspace's fixed-point maths and the f32 a device wants, and every cast here is that narrowing rather than an oversight; nothing downstream of it is hashed, sent or replayed, so the rounding costs nothing a golden could see"
)]

/// The identity.
pub use corvid_glm::IDENTITY;
use corvid_glm::Mat4;
use corvid_rotation::Basis;
use corvid_shape::Frustum;
use corvid_transform::{GlobalFineTransform, Transform};
use corvid_vector::{GlobalFinePoint, GlobalPoint};

/// A rotation as a matrix, with no translation.
const fn from_basis(basis: Basis) -> Mat4 {
    let r = basis.to_rows();
    Mat4::new(
        r[0][0].to_f32(),
        r[0][1].to_f32(),
        r[0][2].to_f32(),
        0.0, //
        r[1][0].to_f32(),
        r[1][1].to_f32(),
        r[1][2].to_f32(),
        0.0, //
        r[2][0].to_f32(),
        r[2][1].to_f32(),
        r[2][2].to_f32(),
        0.0, //
        0.0,
        0.0,
        0.0,
        1.0,
    )
}

/// Where an instance sits **relative to the eye**, and which way it faces.
///
/// The subtraction is in `f64` and the result is `f32`, which is the whole
/// point of the module: an instance ten metres from a camera that is 1e13 m
/// from the origin still resolves to fractions of a millimetre.
#[must_use]
pub fn model(transform: Transform, eye: GlobalFinePoint) -> Mat4 {
    let mut out = from_basis(transform.basis());
    let position: GlobalPoint = transform.position();
    let here = position.to_array();
    let there = eye.to_array();
    for axis in 0..3 {
        out[(axis, 3)] = (here[axis].to_f64() - there[axis].to_f64()) as f32;
    }
    out
}

/// What is left of a view transform once [`model`] has taken the eye's
/// position out: the inverse of the camera's rotation.
///
/// The inverse of a rotation is its transpose, so this costs a transpose and
/// nothing else. Composing the camera's rotation rather than inverting it
/// turns the world the wrong way, which looks like a camera whose controls are
/// mirrored rather than like anything failing.
#[must_use]
pub fn view(camera: GlobalFineTransform) -> Mat4 {
    from_basis(camera.basis()).transpose()
}

/// The projection matrix a [`Frustum`] describes.
///
/// **One formula, for both kinds of frustum.** The half-height at a forward
/// distance `d` is `h(d) = base + slope * d`, so setting the homogeneous `w`
/// to `slope * d + base` — which is linear in `d`, and therefore a legal
/// matrix row — makes the perspective divide produce
/// `ndc = z / h(d)` whatever the frustum is. A perspective frustum is
/// `base == 0` and an orthographic box is `slope == 0`; neither is a branch
/// here, and orthographic is exact rather than a very long frustum
/// approximated.
///
/// The depth row follows from the same two numbers: mapping `near` to zero and
/// `far` to one needs `alpha * d - alpha * near` over `h(d)`, with
/// `alpha = h(far) / (far - near)`.
///
/// The clip convention is the one every backend `wgpu` targets agrees on: `x`
/// right, `y` up, `z` from zero at the near plane to one at the far plane. The
/// camera convention is the workspace's: `x` right, `y` forward, `z` up. The
/// swap between the two happens in this matrix's rows, which is why row one
/// reads from `z` and row three reads from `y`.
///
/// ```
/// use corvid_camera::matrix;
/// use corvid_shape::Frustum;
///
/// let same = matrix::projection(Frustum::default(), 16.0 / 9.0);
/// assert_eq!(same, matrix::projection(Frustum::default(), 16.0 / 9.0));
/// ```
#[must_use]
pub fn projection(frustum: Frustum, aspect: f32) -> Mat4 {
    let aspect = sane(aspect);
    let base = frustum.base.to_f32();
    let slope = frustum.slope.to_f32();
    let near = frustum.near.to_f32();
    let far = frustum.far.to_f32();

    // A frustum with no extent at all — a field of view of zero, or a box of
    // no height — projects everything onto a point. Left alone it would put a
    // zero in `w` and a `NaN` in every vertex the matrix touched, which a
    // device rasterises into something arbitrary. Collapsing the two spatial
    // rows instead draws nothing, which is what the degenerate case meant
    // before this was one formula.
    if base == 0.0 && slope == 0.0 {
        return Mat4::new(
            0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        );
    }

    // `h(far) / (far - near)`, or zero for a frustum with no depth to it.
    let alpha = depth_range(near, far) * (slope * far + base);

    Mat4::new(
        1.0 / aspect,
        0.0,
        0.0,
        0.0, //
        0.0,
        0.0,
        1.0,
        0.0, //
        0.0,
        alpha,
        0.0,
        -alpha * near, //
        0.0,
        slope,
        0.0,
        base,
    )
}

/// An aspect ratio that will not put an infinity in a matrix.
///
/// A viewport with a zero dimension has no aspect ratio; one is as good an
/// answer as any, and what would otherwise be drawn is nothing.
///
/// Public because a game that reconstructs a ray per pixel — a sky shader is
/// the usual one — has to derive its frustum's half-extents from the same
/// aspect this module built the projection from, or its horizon does not sit
/// where the geometry's does. Two copies of this three-line guard is two
/// answers to one question.
#[must_use]
pub fn sane(aspect: f32) -> f32 {
    if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        1.0
    }
}

/// `1 / (far - near)`, or zero for a frustum with no depth to it.
///
/// A `near` equal to `far` is a division by zero, and an infinity in a matrix
/// makes every vertex it touches a `NaN` that the device then rasterises into
/// something arbitrary. Nothing checks a game's frustum before it gets here, so
/// this is where a degenerate one is caught.
///
/// What zero leaves is a depth row of zeroes, which is not the same as drawing
/// nothing: every vertex arrives at the near plane with `x` and `y` intact, so
/// against the `Clear(1.0)` and `Less` this workspace draws with, the first
/// primitive to cover a pixel keeps it and everything behind *and in front of*
/// it is rejected. The picture is drawn in submission order rather than depth
/// order. That is a wrong picture rather than an absent one, and it is the
/// price of catching the division here instead of refusing the frustum;
/// [`projection`]'s own guard does collapse the matrix.
#[must_use]
pub fn depth_range(near: f32, far: f32) -> f32 {
    let span = far - near;
    if corvid_float::abs(span) > f32::EPSILON {
        1.0 / span
    } else {
        0.0
    }
}
