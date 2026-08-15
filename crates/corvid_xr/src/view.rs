//! The two eyes, and the matrices they want.
//!
//! The matrices are `corvid_camera`'s, and so is the [`Eye`] they go into.
//! What is here is the one thing that crate cannot express: an XR frustum is
//! **asymmetric**, because a headset's lens is not centred on the eye, and a
//! `Frustum` is four numbers describing a symmetric one. So the projection
//! below is built from four half-angles rather than from a frustum, and
//! everything either side of it is shared.
//!
//! The eye's position is split into whole metres, which stay integers, and a
//! sub-metre remainder folded into the matrix -- so every `f32` downstream sees
//! a difference rather than an absolute, which is what makes a hand a metre
//! away solid while the planet under it is ten thousand kilometres across.

use corvid_camera::{Eye, matrix};

use corvid_fixed::{Angle16, I16F16};
use corvid_glm::{Mat4, Vec3i};
use corvid_vector::FinePoint;
use serde::{Deserialize, Serialize};

use crate::{Anchor, Pose, Side};

/// One eye: where it is, and the four half-angles of its frustum.
///
/// Four angles rather than a field of view and an aspect, because an XR frustum
/// is asymmetric -- the runtime's lens is not centred on the eye, and a
/// symmetric projection would crop. [`left`](Self::left) and
/// [`down`](Self::down) are the ones that read as negative through
/// [`Angle16::to_signed_radians`].
///
/// ```
/// use corvid_xr::EyeView;
/// use corvid_fixed::Angle16;
///
/// let eye = EyeView::default();
/// assert!(eye.left.to_signed_radians() < 0.0);
/// assert!(eye.right.to_signed_radians() > 0.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EyeView {
    /// Where the eye is, in stage space.
    pub pose: Pose,
    /// The half-angle to the left of the eye's forward axis. Negative.
    pub left: Angle16,
    /// The half-angle to the right of it. Positive.
    pub right: Angle16,
    /// The half-angle above it. Positive.
    pub up: Angle16,
    /// The half-angle below it. Negative.
    pub down: Angle16,
}

impl Default for EyeView {
    /// At the stage origin, with the roughly 90 deg-by-90 deg asymmetric frustum a
    /// consumer headset offers.
    #[inline]
    fn default() -> Self {
        Self {
            pose: Pose::IDENTITY,
            left: Angle16::from_degrees(-50.0),
            right: Angle16::from_degrees(45.0),
            up: Angle16::from_degrees(45.0),
            down: Angle16::from_degrees(-45.0),
        }
    }
}

impl EyeView {
    /// The eye at `pose`, with this frustum.
    #[must_use]
    #[inline]
    pub const fn at(self, pose: Pose) -> Self {
        Self { pose, ..self }
    }

    /// The four half-angles as their tangents, left, right, up, down.
    ///
    /// A half-angle at or past a quarter turn has no tangent a frustum could
    /// use, and one that is exactly zero collapses the frustum; both come back
    /// as the symmetric 45 deg default's tangent so that a degenerate frustum
    /// draws something wrong rather than filling the matrix with infinities and
    /// every vertex with `NaN`.
    #[must_use]
    pub fn tangents(self) -> [f32; 4] {
        [self.left, self.right, self.up, self.down].map(|angle| {
            let tangent = corvid_float::wide::tan(angle.to_signed_radians());
            if tangent.is_finite() && corvid_float::wide::abs(tangent) > f64::EPSILON {
                corvid_float::demote(tangent)
            } else {
                1.0
            }
        })
    }

    /// The clip matrix this eye wants, relative to its own position.
    ///
    /// Projection times view: it takes a position **already expressed relative
    /// to the eye**, in world axes, and puts it in clip space. The convention
    /// is the workspace's -- `x` right, `y` up, `z` from zero at the near plane
    /// to one at the far -- from the camera axes `x` right, `y` forward, `z` up.
    #[must_use]
    pub fn clip(self, near: I16F16, far: I16F16) -> Mat4 {
        self.projection(near, far) * matrix::view(self.pose.to_fine())
    }

    /// The asymmetric perspective projection alone, from eye space to clip
    /// space.
    #[must_use]
    pub fn projection(self, near: I16F16, far: I16F16) -> Mat4 {
        let [tl, tr, tu, td] = self.tangents();
        let width = tr - tl;
        let height = tu - td;
        // A frustum with no width or no height projects everything onto a line,
        // and one over zero is an infinity that spreads through a whole row.
        let horizontal = span(width);
        let vertical = span(height);
        let near = near.to_f32();
        let far = far.to_f32();
        let range = matrix::depth_range(near, far);
        // Written across and stored down: `Mat4::new` takes rows and nalgebra
        // keeps columns, which is the order a WGSL `mat4x4` reads.
        Mat4::new(
            2.0 * horizontal,
            -(tr + tl) * horizontal,
            0.0,
            0.0, //
            0.0,
            -(tu + td) * vertical,
            2.0 * vertical,
            0.0, //
            0.0,
            far * range,
            0.0,
            -near * far * range, //
            0.0,
            1.0,
            0.0,
            0.0,
        )
    }

    /// This eye, in the world, as the matrices a device binds.
    ///
    /// The position is split: [`Eye::coarse`] is the whole-metre part, taken as
    /// a floor so the remainder is never negative, and the remainder is folded
    /// into [`Eye::clip`]. That split is what the fine tier was for -- the `f32`
    /// mantissa is spent on the metres near the eye rather than on the distance
    /// to the origin.
    #[must_use]
    pub fn eye(self, anchor: Anchor, near: I16F16, far: I16F16) -> Eye {
        let world = anchor.to_world(self.pose);
        let mut coarse = Vec3i::zeros();
        let mut remainder = corvid_glm::IDENTITY;
        for (axis, component) in world.position().to_array().iter().enumerate() {
            // The split is `corvid_fixed`'s: a floor rather than a truncation,
            // so the remainder is in `[0, 1)` for a negative position as well
            // as a positive one, and the pair still sums to the position when
            // the whole part saturates.
            let (whole, fine) = component.split_floor();
            coarse[axis] = whole;
            remainder[(axis, 3)] = -fine.to_f32();
        }
        Eye {
            coarse,
            _pad: 0,
            clip: self.projection(near, far) * matrix::view(world) * remainder,
        }
    }
}

/// Both eyes, this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Views {
    /// The left eye.
    pub left: EyeView,
    /// The right eye.
    pub right: EyeView,
}

impl Views {
    /// Both eyes a fixed separation apart, looking the way `head` looks.
    ///
    /// `separation` is the whole interpupillary distance, so each eye sits half
    /// of it to its own side of the head, along the head's own right axis.
    #[must_use]
    pub fn from_head(head: Pose, separation: I16F16, frustum: EyeView) -> Self {
        // An interpupillary distance is written in the near tier and a stage
        // pose is in it, so the halving and the rotation both happen there and
        // nothing is widened on the way.
        let half = separation.saturating_div(I16F16::from(2));
        let offset = head
            .basis()
            .rotate_fine(FinePoint::new(half, I16F16::ZERO, I16F16::ZERO));
        Self {
            left: frustum.at(head.with_position(head.position().sub(offset))),
            right: frustum.at(head.with_position(head.position().add(offset))),
        }
    }

    /// One eye, by side.
    #[must_use]
    #[inline]
    pub const fn eye(self, side: Side) -> EyeView {
        match side {
            Side::Left => self.left,
            Side::Right => self.right,
        }
    }

    /// Both, left first.
    #[must_use]
    #[inline]
    pub const fn to_array(self) -> [EyeView; 2] {
        [self.left, self.right]
    }
}

impl From<[EyeView; 2]> for Views {
    #[inline]
    fn from([left, right]: [EyeView; 2]) -> Self {
        Self { left, right }
    }
}

impl From<Views> for [EyeView; 2] {
    #[inline]
    fn from(views: Views) -> Self {
        views.to_array()
    }
}

/// `2 / extent`, or one for a frustum with no extent to it.
fn span(extent: f32) -> f32 {
    if extent.is_finite() && extent.abs() > f32::EPSILON {
        1.0 / extent
    } else {
        1.0
    }
}
