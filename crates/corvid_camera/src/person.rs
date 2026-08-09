//! A camera that stands where it is and walks.

use crate::Camera;
use corvid_fixed::{Angle32, I24F8, Pitch32};
use corvid_rotation::{FineRotation, Versor};
use corvid_shape::Frustum;
use corvid_transform::FineTransform;
use corvid_vector::{Direction, GlobalPoint};

/// A camera at a position, steered by yaw and pitch, walked in its own frame.
///
/// The first-person camera: a shooter's, a walking simulator's, and the example
/// game's defender standing on the surface of a planet. It holds no velocity
/// and no collision — those are a game's, and they belong in its `State` where
/// every peer agrees on them. This is what the player *looks* through, which is
/// client-local and is a function of one machine's frame rate.
///
/// It shares [`Orbit`](crate::Orbit)'s steering exactly, including the reason
/// for it: [`turn`](Self::turn) decodes, adds and rebuilds through
/// [`Versor::from_yaw_pitch_roll`] rather than composing two half-angle
/// quaternions, because the composition has a reject branch that 46% of the
/// representable yaws take.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct FirstPerson {
    /// Where the eye is.
    pub position: GlobalPoint,
    /// The word `#[repr(C)]` would otherwise insert here unnamed. Always zero,
    /// for the reason [`Orbit::_pad`](crate::Orbit::_pad) gives.
    #[expect(
        clippy::pub_underscore_fields,
        reason = "the leading underscore is the name: it says the word is padding rather than a field a caller sets, and it has to be public because bytemuck::Pod requires every field to be constructible"
    )]
    pub _pad: u32,
    /// Which way it faces, packed.
    pub facing: FineRotation,
    /// How far up or down it will look, either side of level.
    ///
    /// Zero on a `Default`, which is not the useful value —
    /// [`new`](Self::new) is what a game calls, and it applies
    /// [`DEFAULT_PITCH_LIMIT`](Self::DEFAULT_PITCH_LIMIT). The `Default`
    /// derive is here because a `Render::View` needs one and a camera that
    /// cannot look up is a better default than one that can look through its
    /// own feet.
    pub pitch_limit: Pitch32,
    /// How much of the world the camera sees.
    pub frustum: Frustum,
    /// The word `#[repr(C)]` would otherwise insert at the end.
    ///
    /// Always zero. The fields above come to forty-four bytes and the
    /// alignment [`facing`](Self::facing) imposes is eight, so the type is
    /// forty-eight whether this is named or not.
    #[expect(
        clippy::pub_underscore_fields,
        reason = "the leading underscore is the name: it says the word is padding rather than a field a caller sets, and it has to be public because bytemuck::Pod requires every field to be constructible"
    )]
    pub _tail: u32,
}

impl FirstPerson {
    /// A quarter turn less a hair: 89°, either side of level.
    ///
    /// Short of straight up, for the reason [`Orbit`](crate::Orbit)'s limit is:
    /// a yaw has no meaning at the pole, so a camera that reached it would spin
    /// on the spot as the player crossed it. Closer to the pole than an orbit's
    /// limit because a first-person camera is expected to look almost straight
    /// up and an orbiting one is not.
    pub const DEFAULT_PITCH_LIMIT: Pitch32 = Pitch32::from_degrees(89.0);

    /// A camera standing at a position, level, facing forward.
    #[must_use]
    #[inline]
    pub const fn new(position: GlobalPoint) -> Self {
        Self {
            position,
            _pad: 0,
            facing: FineRotation::IDENTITY,
            pitch_limit: Self::DEFAULT_PITCH_LIMIT,
            frustum: Frustum::DEFAULT,
            _tail: 0,
        }
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

    /// Turns the camera, clamping the pitch.
    pub const fn turn(&mut self, yaw: Angle32, pitch: Pitch32) {
        let (current_yaw, current_pitch, _roll) = self.facing.to_versor().to_yaw_pitch_roll();
        let turned = current_yaw.wrapping_add(yaw);
        let raised = current_pitch
            .saturating_add(pitch)
            .clamp(self.pitch_limit.neg(), self.pitch_limit);
        self.facing =
            FineRotation::from_versor(Versor::from_yaw_pitch_roll(turned, raised, Angle32::ZERO));
    }

    /// Moves the camera in its own frame: forward, right and up, in metres.
    ///
    /// Looking down and walking forward walks *downwards*, which is a flying
    /// camera and is what this is for. [`walk_level`](Self::walk_level) is the
    /// one a character on the ground calls.
    pub fn walk(&mut self, forward: I24F8, right: I24F8, up: I24F8) {
        let step = GlobalPoint::from_array([right, forward, up]);
        self.position += self.facing.to_versor().rotate_global(step);
    }

    /// The same, with the pitch dropped, so looking down does not walk into the
    /// floor.
    ///
    /// The one a character controller calls. Yaw alone is applied, so forward
    /// stays on the horizontal plane however far down the player is looking —
    /// which is what every game with feet does, and getting it wrong is the
    /// classic bug where sprinting while looking at the ground stops you dead.
    pub fn walk_level(&mut self, forward: I24F8, right: I24F8) {
        let (yaw, _pitch) = self.angles();
        let level = Versor::from_yaw_pitch_roll(yaw, Pitch32::ZERO, Angle32::ZERO);
        let step = GlobalPoint::from_array([right, forward, I24F8::ZERO]);
        self.position += level.rotate_global(step);
    }

    /// Which way the eye faces.
    #[must_use]
    #[inline]
    pub const fn orientation(self) -> Versor {
        self.facing.to_versor()
    }

    /// How far round and how far up the eye has been turned.
    #[must_use]
    #[inline]
    pub const fn angles(self) -> (Angle32, Pitch32) {
        let (yaw, pitch, _roll) = self.facing.to_versor().to_yaw_pitch_roll();
        (yaw, pitch)
    }

    /// Which way is forward, as a unit direction.
    #[must_use]
    #[inline]
    pub const fn forward(self) -> Direction {
        self.orientation().forward()
    }

    /// Where the eye is and which way it faces.
    #[must_use]
    #[inline]
    pub const fn pose(self) -> FineTransform {
        FineTransform::new(self.position.to_global_fine(), self.facing)
    }
}

/// Where the eye is and which way it faces, which is [`FirstPerson::pose`].
impl From<FirstPerson> for FineTransform {
    #[inline]
    fn from(camera: FirstPerson) -> Self {
        camera.pose()
    }
}

/// A first-person camera is a pose and a frustum, which is what a camera is.
/// The eye this first-person camera is looking through.
///
/// First-person camera is a way of working out where a camera should be; this is the
/// answer it gives, and it is what `Controller::look` hands to a renderer.
impl FirstPerson {
    /// Where this first-person camera puts the eye, and how much it sees.
    #[must_use]
    #[inline]
    pub const fn camera(&self) -> Camera {
        Camera::new(Self::pose(*self), self.frustum)
    }
}
