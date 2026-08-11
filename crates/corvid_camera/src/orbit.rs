//! A camera that watches a thing from a fixed distance.

use crate::Camera;
use corvid_fixed::{Angle32, Factor32, I24F8, Pitch32};
use corvid_rotation::{FineRotation, Versor};
use corvid_shape::Frustum;
use corvid_transform::FineTransform;
use corvid_vector::GlobalPoint;

/// A camera on a sphere about an anchor, steered by yaw and pitch.
///
/// The third-person camera: the one a strategy game, a builder and the
/// example game's defender all want. It is composed into a
/// `Render::View`, advanced in `look`, and never seen by the simulation --
/// which is the whole of why a camera may read a wall clock and an action may
/// not.
///
/// # Two properties the steering holds, and why
///
/// **Adjacent yaws are adjacent.** [`turn`](Self::turn) builds its rotation
/// with [`Versor::from_yaw_pitch_roll`], which multiplies the basis out in Q30
/// and has no reject branch. Composing two half-angle quaternions instead would
/// go through [`Versor::from_xyzw`], which rejects anything further from unit
/// than 1.5e-5 -- and a sine and a cosine from
/// [`Angle16::sin_cos`](corvid_fixed::Angle16::sin_cos) miss
/// `sin^2 + cos^2 = 1` by up to 4.3e-5, so 46% of the 65 536 representable yaws
/// have no versor to build and nothing sensible to rotate by but
/// [`Versor::IDENTITY`], which is a camera that snaps back to facing forward on
/// nearly every other angle.
///
/// **The orbit is rigid and the anchor is what lags.**
/// [`ease_towards`](Self::ease_towards) moves the anchor and nothing else, and
/// [`eye_position`](Self::eye_position) is derived from it exactly every
/// frame. Easing the eye
/// while the facing was immediate would be the same camera described twice and
/// never at the same moment: at 81 degrees/s -- a two-pixel drag -- the two are 138 degrees
/// apart, which is an empty screen, and a faster spin pulls the eye inward far
/// enough to sit *inside* what it is watching, where back-face culling draws
/// nothing at all. The player still sees a camera that eases and settles,
/// because it eases towards the thing that is actually moving, and the framing
/// does not depend on how fast the mouse is going or how fast the display is.
///
/// `tests/orbit.rs` freezes both.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct Orbit {
    /// What the camera is watching.
    ///
    /// The eased quantity, and the only one.
    pub anchor: GlobalPoint,
    /// The word `#[repr(C)]` would otherwise insert here unnamed.
    ///
    /// Always zero. [`facing`](Self::facing) is eight bytes and wants an
    /// eight-byte boundary, and the three four-byte components above it stop
    /// one short of one -- so something goes here either way. `bytemuck::Pod`
    /// forbids a struct with padding a reader cannot see, and naming it is
    /// also what stops somebody later putting a field in the gap and changing
    /// every serialized camera in the process.
    #[expect(
        clippy::pub_underscore_fields,
        reason = "the leading underscore is the name: it says the word is padding rather than a field a caller sets, and it has to be public because bytemuck::Pod requires every field to be constructible"
    )]
    pub _pad: u32,
    /// Which way the eye faces, packed.
    ///
    /// A [`FineRotation`] rather than a yaw and a pitch, for three reasons. It
    /// is what [`pose`](Self::pose) has to hand a [`FineTransform`] anyway, so
    /// storing it makes that a move. One rotation is one bit pattern, because
    /// the sign is canonicalized -- so the double cover cannot give one facing
    /// two patterns that compare unequal. And it cannot represent a
    /// non-rotation, where a yaw and a pitch are two independent numbers with a
    /// composition step between them and the thing they mean -- and that step is
    /// the one the property above is about.
    pub facing: FineRotation,
    /// Where the eye sits relative to the anchor, **in the camera's own
    /// frame**.
    ///
    /// An offset rather than a scalar distance, because a third-person camera
    /// almost always wants to sit a little above what it is watching as well as
    /// behind it -- and a rise expressed as a separate field would be a second
    /// number that can disagree with the first about where the eye is.
    /// [`new`](Self::new) is the common case, which puts it straight behind at
    /// a given distance; [`with_offset`](Self::with_offset) is the general one.
    pub offset: GlobalPoint,
    /// How far up or down the eye will go, either side of level.
    ///
    /// [`DEFAULT_PITCH_LIMIT`](Self::DEFAULT_PITCH_LIMIT) explains the value.
    pub pitch_limit: Pitch32,
    /// How much of the world the camera sees.
    ///
    /// The camera owns it rather than being handed one per frame, which is
    /// what makes [`camera`](Self::camera) answerable from the orbit alone
    /// rather than from the orbit and an argument that has to agree with it.
    pub frustum: Frustum,
}

impl Orbit {
    /// A fifth of a turn: 72 degrees, either side of level.
    ///
    /// Short of the pole on purpose. A camera that reached straight up has no
    /// well-defined yaw there and spins on the spot as the player crosses it.
    pub const DEFAULT_PITCH_LIMIT: Pitch32 = Pitch32::from_turns(0.2);

    /// Eight metres, which frames a human-scale thing at a desk.
    pub const DEFAULT_DISTANCE: I24F8 = I24F8::from_f64(8.0);

    /// A camera at the origin, level, facing forward, a given distance behind
    /// what it is watching.
    #[must_use]
    #[inline]
    pub const fn new(distance: I24F8) -> Self {
        Self {
            anchor: GlobalPoint::ZERO,
            _pad: 0,
            facing: FineRotation::IDENTITY,
            offset: GlobalPoint::new(I24F8::ZERO, distance.saturating_neg(), I24F8::ZERO),
            pitch_limit: Self::DEFAULT_PITCH_LIMIT,
            frustum: Frustum::DEFAULT,
        }
    }

    /// The same camera with the eye somewhere else relative to the anchor.
    ///
    /// The offset is in the camera's own frame: -Y is behind, +Z is above, so a
    /// camera ten metres back and a metre and a half up is `(0, -10, 1.5)`.
    ///
    /// ```
    /// use corvid_camera::Orbit;
    /// use corvid_fixed::I24F8;
    /// use corvid_vector::{GlobalPoint, globalpoint};
    ///
    /// let back_and_up = GlobalPoint::new(
    ///     I24F8::ZERO,
    ///     I24F8::from_f64(-10.0),
    ///     I24F8::from_f64(1.5),
    /// );
    /// let camera = Orbit::default().with_offset(back_and_up);
    ///
    /// // Level and facing forward, so the eye is exactly where the offset put it.
    /// assert_eq!(camera.eye_position(), back_and_up);
    /// assert_eq!(camera.anchor, globalpoint(0, 0, 0));
    /// ```
    #[must_use]
    #[inline]
    pub const fn with_offset(self, offset: GlobalPoint) -> Self {
        Self { offset, ..self }
    }

    /// How far the eye is from the anchor.
    ///
    /// Derived from [`offset`](Self::offset) rather than stored beside it, so
    /// there is one answer to "how far back is this camera" instead of two that
    /// can disagree.
    #[must_use]
    #[inline]
    pub const fn distance(self) -> I24F8 {
        self.offset.length()
    }

    /// The same camera at a different pitch limit.
    #[must_use]
    #[inline]
    pub const fn with_pitch_limit(self, pitch_limit: Pitch32) -> Self {
        Self {
            pitch_limit,
            ..self
        }
    }

    /// Swings the camera round and up, clamping the pitch.
    ///
    /// The yaw wraps and the pitch does not, which is the difference between
    /// the two types: an [`Angle32`] is a turn and a [`Pitch32`] is a signed
    /// elevation with ends.
    ///
    /// # Why this decodes and re-encodes rather than composing
    ///
    /// The facing is stored packed, so steering it is a read-modify-write
    /// through a lossy encoding: the yaw and pitch come out of the versor, the
    /// deltas are added, and a fresh versor is built with roll zero. That costs
    /// the packing's own precision each call -- about 0.0034 degrees at worst, which is
    /// far below a mouse's smallest movement -- and it buys the property the
    /// type documentation is about: there is one facing rather than two numbers
    /// that can disagree with it.
    pub const fn turn(&mut self, yaw: Angle32, pitch: Pitch32) {
        let (current_yaw, current_pitch, _roll) = self.facing.to_versor().to_yaw_pitch_roll();
        let turned = current_yaw.wrapping_add(yaw);
        let raised = current_pitch
            .saturating_add(pitch)
            .clamp(self.pitch_limit.neg(), self.pitch_limit);
        self.facing =
            FineRotation::from_versor(Versor::from_yaw_pitch_roll(turned, raised, Angle32::ZERO));
    }

    /// Moves the anchor a fraction of the way towards a target.
    ///
    /// Exact at both ends: at [`Factor32::ZERO`] nothing moves and at
    /// [`Factor32::ONE`] it arrives, bit for bit, which is what every
    /// interpolation in this workspace owes.
    ///
    /// **The anchor and nothing else.** See the type's own documentation for
    /// why the eye is not the thing that lags.
    #[inline]
    pub const fn ease_towards(&mut self, target: GlobalPoint, weight: Factor32) {
        self.anchor = self.anchor.lerp(target, weight);
    }

    /// Which way the eye faces.
    #[must_use]
    #[inline]
    pub const fn orientation(self) -> Versor {
        self.facing.to_versor()
    }

    /// How far round and how far up the eye has been swung.
    ///
    /// Decoded from [`facing`](Self::facing) rather than stored beside it, so
    /// there is one answer to "which way is this camera pointing" instead of
    /// two that can disagree. The roll is dropped rather than returned: this
    /// camera never builds one, so a non-zero value here would be rounding
    /// rather than a fact.
    #[must_use]
    #[inline]
    pub const fn angles(self) -> (Angle32, Pitch32) {
        let (yaw, pitch, _roll) = self.facing.to_versor().to_yaw_pitch_roll();
        (yaw, pitch)
    }

    /// Where the eye belongs, given what it is watching.
    ///
    /// The camera's own [`offset`](Self::offset) turned by its own
    /// orientation -- so swinging the mouse carries the eye round the anchor
    /// rather than turning it on the spot.
    #[must_use]
    #[inline]
    pub fn perch(self, at: GlobalPoint) -> GlobalPoint {
        at + self.orientation().rotate_global(self.offset)
    }

    /// Where the eye is.
    ///
    /// Named for the position rather than for the eye, because
    /// [`Camera::eye`](crate::Camera::eye) is the uniform block a device
    /// binds and one name cannot be both.
    #[must_use]
    #[inline]
    pub fn eye_position(self) -> GlobalPoint {
        self.perch(self.anchor)
    }

    /// Where the eye is and which way it faces.
    ///
    /// The rotation is a move rather than a conversion, which is the immediate
    /// reason [`facing`](Self::facing) is stored packed.
    #[must_use]
    #[inline]
    pub fn pose(self) -> FineTransform {
        FineTransform::new(self.eye_position().to_global_fine(), self.facing)
    }

    /// Where this orbit camera puts the eye, and how much it sees.
    ///
    /// An orbit is a way of working out where a camera should be; a [`Camera`]
    /// is the answer it gives, and it is what `Controller::look` hands to a
    /// renderer.
    #[must_use]
    #[inline]
    pub fn camera(self) -> Camera {
        Camera::new(self.pose(), self.frustum)
    }
}

/// Watching the origin, from eight metres behind it, and level.
///
/// A `Render::View` is built by `Default` before the first tick and before the
/// first displayed frame, so a camera whose default is not a sensible starting
/// camera draws its first frame from wherever `Default` put it -- and that first
/// frame is the one a screenshot test takes.
impl Default for Orbit {
    #[inline]
    fn default() -> Self {
        Self::new(Self::DEFAULT_DISTANCE)
    }
}

/// Where the eye is and which way it faces, which is [`Orbit::pose`].
///
/// Total: an orbit always has a pose, because the eye is derived from the
/// anchor and the facing rather than stored beside them.
impl From<Orbit> for FineTransform {
    #[inline]
    fn from(camera: Orbit) -> Self {
        camera.pose()
    }
}
