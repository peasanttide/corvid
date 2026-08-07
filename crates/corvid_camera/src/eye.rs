//! The whole camera, as the bytes a uniform buffer takes.

use crate::matrix;
use corvid_fixed::I48F16;
use corvid_glm::{IDENTITY, Mat4};
use corvid_shape::Frustum;
use corvid_transform::GlobalFineTransform;

/// The whole camera, as the bytes a uniform buffer takes.
///
/// The eye's position is split in two: [`coarse`](Self::coarse) is whole
/// metres, integer and exact, and [`clip`](Self::clip) is view times projection
/// **relative to it**. So every `f32` in the matrix sees a difference rather
/// than an absolute, and the precision follows the camera.
///
/// That is what makes a cube a metre away solid while the planet under it is
/// ten thousand kilometres across. A game writes world positions into its
/// vertex stage as offsets from `coarse` — a subtraction it does in integers,
/// where it is free — and multiplies by `clip`.
///
/// ```
/// use corvid_camera::Eye;
/// use corvid_shape::Frustum;
/// use corvid_transform::GlobalFineTransform;
/// use corvid_vector::globalfinepoint;
///
/// let far_away = GlobalFineTransform::new(globalfinepoint(10_000_000, 0, 0), Default::default());
/// let eye = Eye::new(far_away, Frustum::default(), 16.0 / 9.0);
/// assert_eq!(eye.coarse, [10_000_000, 0, 0]);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Eye {
    /// Where the eye is, in whole metres.
    ///
    /// Integer and exact, and it is what a game subtracts from a world position
    /// before that position is allowed to become an `f32`.
    pub coarse: [i32; 3],
    /// The fourth component a `vec3<i32>` has in a uniform block anyway.
    ///
    /// Always zero. It is here rather than implied because `#[repr(C)]` is what
    /// this type is for, and a padding word a reader cannot see is a padding
    /// word somebody eventually writes a field into.
    #[expect(
        clippy::pub_underscore_fields,
        reason = "the leading underscore is the name: it says the word is padding rather than a field a caller sets, and it has to be public because bytemuck::Pod requires every field to be constructible"
    )]
    pub _pad: i32,
    /// View times projection, **relative to [`coarse`](Self::coarse)**.
    ///
    /// Column-major, which is what WGSL's `mat4x4` reads and what
    /// `corvid_glm` stores. It carries the sub-metre remainder of the eye's
    /// position, so composing it with a model matrix that has already
    /// subtracted `coarse` gives the whole transform.
    pub clip: Mat4,
}

impl Eye {
    /// The camera at `pose`, seeing `frustum`, on a viewport of `aspect`.
    ///
    /// [`coarse`](Self::coarse) is the whole-metre part of the position, taken
    /// as a floor rather than a rounding so that the remainder is never
    /// negative; [`clip`](Self::clip) is the projection times the view times
    /// that remainder's translation.
    ///
    /// The pair is the point. A game subtracts `coarse` from a world position
    /// in integers — where it is exact and free — and hands the difference to
    /// its vertex stage as an `f32`, so the twenty-four bits of mantissa are
    /// spent on the metres near the camera rather than on the distance to the
    /// origin.
    ///
    /// A camera further than 2.1e9 m from the origin has a coarse position
    /// that does not fit an `i32`; it saturates, and past that the remainder is
    /// what carries the difference and the precision goes back to being an
    /// `f32`'s. That is two million kilometres out, against the 8388 km a
    /// `GlobalPoint` reaches at all.
    #[must_use]
    pub fn new(pose: GlobalFineTransform, frustum: Frustum, aspect: f32) -> Self {
        let position = pose.position().to_array();
        let mut coarse = [0i32; 3];
        let mut remainder = IDENTITY;
        for (axis, component) in position.iter().enumerate() {
            let bits = component.to_bits();
            // An arithmetic shift is a floor, so the remainder below is in
            // `[0, 1)` for a negative position as well as a positive one.
            let whole = i32::try_from(bits >> I48F16::FRAC_BITS).unwrap_or(if bits < 0 {
                i32::MIN
            } else {
                i32::MAX
            });
            coarse[axis] = whole;
            let fine = bits - (i64::from(whole) << I48F16::FRAC_BITS);
            #[expect(
                clippy::cast_precision_loss,
                reason = "the remainder is under one metre in I48F16 steps, which is sixteen bits against an f32 mantissa's twenty-four — the conversion is exact"
            )]
            {
                remainder[(axis, 3)] = -(fine as f32) / FINE;
            }
        }

        Self {
            coarse,
            _pad: 0,
            clip: matrix::projection(frustum, aspect) * matrix::view(pose) * remainder,
        }
    }
}

/// How many steps an [`I48F16`] has to the metre, as the `f32` the remainder
/// above is divided by.
#[expect(
    clippy::cast_precision_loss,
    reason = "a power of two below 2^24 converts to f32 exactly"
)]
const FINE: f32 = (1u32 << I48F16::FRAC_BITS) as f32;
