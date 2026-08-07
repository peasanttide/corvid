//! What every camera has, and what follows from it.

use crate::{Eye, matrix};
use corvid_fixed::{I16F16, Signed32};
use corvid_glm::Mat4;
use corvid_rotation::Versor;
use corvid_shape::{Frustum, Ray};
use corvid_transform::GlobalFineTransform;
use corvid_vector::{Direction, FinePoint, GlobalPoint};

/// A pose and a frustum: where the camera is, and how much it sees.
///
/// Those two are all a camera *is*. Everything else a renderer asks of one —
/// the clip matrix, the uniform block, the ray under the cursor — follows from
/// them by arithmetic, so it is written once here rather than once per camera.
///
/// [`Orbit`](crate::Orbit) and [`FirstPerson`](crate::FirstPerson) **produce**
/// one of these. They are ways of working out where an eye should be; this is
/// the answer they give.
///
/// # Why this is a struct and not a trait
///
/// It was a trait, whose whole surface was `pose()` and `frustum()` plus three
/// derivations of them. Three separate contracts have to name this type —
/// `Controller::look` answers one, `Render::draw` and `Auralizer::hear` are
/// handed one — and a trait would have meant either a type parameter threaded
/// through every one of them and through the `App`'s bounds, or a
/// `Box<dyn Camera>` allocated once per displayed frame.
///
/// Nothing was lost. A game with its own idea of how a camera moves writes its
/// own type with its own state and answers one of these, which is what
/// implementing the trait amounted to.
///
/// ```
/// use corvid_camera::{Camera, FirstPerson};
/// use corvid_shape::Frustum;
/// use corvid_vector::globalpoint;
///
/// let camera = FirstPerson::new(globalpoint(0, 0, 2)).camera();
///
/// // The frustum comes with the camera rather than from a second argument.
/// assert_eq!(camera.frustum, Frustum::default());
///
/// // And the matrix follows from the pair.
/// let clip = camera.clip(16.0 / 9.0);
/// assert!(clip.iter().all(|cell| cell.is_finite()));
///
/// // Two fields, so one can be written down directly.
/// assert_eq!(Camera::new(camera.pose, camera.frustum), camera);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Camera {
    /// Where the camera is and which way it faces. World space.
    pub pose: GlobalFineTransform,
    /// How much of the world it sees.
    pub frustum: Frustum,
}

impl Camera {
    /// An eye at a pose, seeing a frustum.
    #[must_use]
    pub const fn new(pose: GlobalFineTransform, frustum: Frustum) -> Self {
        Self { pose, frustum }
    }

    /// Projection times view, **relative to the camera's own position**.
    ///
    /// The translation is not in here. It is in [`eye`](Self::eye), split into
    /// a whole-metre part a game subtracts in integers and a sub-metre
    /// remainder — which is what keeps an `f32` matrix usable at planetary
    /// distance. This is the rotation and the projection alone, for a caller
    /// that is composing its own model matrices with
    /// [`matrix::model`](crate::matrix::model).
    #[must_use]
    pub fn clip(&self, aspect: f32) -> Mat4 {
        matrix::projection(self.frustum, aspect) * matrix::view(self.pose)
    }

    /// The whole camera as the bytes a uniform buffer takes.
    ///
    /// This is the one a game writes into its uniform block every frame.
    #[must_use]
    pub fn eye(&self, aspect: f32) -> Eye {
        Eye::new(self.pose, self.frustum, aspect)
    }

    /// The world-space ray a normalised device position denotes.
    ///
    /// `ndc` runs from `(-1, -1)` at the bottom left to `(1, 1)` at the top
    /// right, which is what a cursor position becomes once the viewport's size
    /// has been divided out. `aspect` is width over height.
    ///
    /// Integer-only, and it names no graphics library — so a picking test
    /// resolves a cursor with no GPU in the process, which is the whole reason
    /// the fixed-point frustum and the `f32` matrix are separate things.
    ///
    /// ```
    /// use corvid_camera::FirstPerson;
    /// use corvid_fixed::{I16F16, Signed32};
    /// use corvid_vector::{Direction, globalpoint};
    ///
    /// let camera = FirstPerson::new(globalpoint(0, 0, 0)).camera();
    /// let centre = camera.ray((Signed32::ZERO, Signed32::ZERO), I16F16::ONE);
    ///
    /// // The middle of the screen is straight ahead, which is +Y here.
    /// assert_eq!(centre.direction, Direction::Y);
    /// ```
    #[must_use]
    pub fn ray(&self, ndc: (Signed32, Signed32), aspect: I16F16) -> Ray {
        let pose = self.pose;
        let frustum = self.frustum;

        // The half-extent one metre ahead. For a perspective frustum that is
        // the slope; for an orthographic box the rays are parallel and the
        // direction does not depend on the screen position at all, which falls
        // out of `slope` being zero.
        let spread = frustum.slope;
        let up = spread.saturating_mul(ndc.1.to_i16f16());
        let right = spread
            .saturating_mul(aspect)
            .saturating_mul(ndc.0.to_i16f16());

        // Forward is one metre, which sets the scale the other two are
        // relative to; normalizing divides it out again.
        //
        // A `FinePoint` and not the `GlobalPoint` the ray's origin is, because
        // this triple is a *ratio* rather than a place: `I24F8`'s 3.9 mm
        // against a forward of one metre would quantize the slope to four parts
        // in a thousand, which at a sixty-degree field of view is a quarter of
        // a degree — five pixels of aim on a 1080-line display. `I16F16` gives
        // fifteen micrometres against the same metre, which is fifty times
        // finer than one pixel. A direction is scale-free, so the units cancel
        // and nothing downstream can tell which type built it.
        let towards = FinePoint::from_array([right, I16F16::ONE, up]);

        // `normalize` answers `None` only for the zero vector, which this is
        // not: the forward component is one whatever the other two are.
        let facing = towards.normalize().unwrap_or(Direction::Y);

        Ray::new(
            pose.position().to_global().unwrap_or(GlobalPoint::ZERO),
            pose.rotation().to_versor().rotate_direction(facing),
        )
    }

    /// Which way the camera faces, as a rotation.
    ///
    /// The pose's rotation, and a convenience: a game orienting something to
    /// match the camera — a billboard, a held tool — wants this and not the
    /// position beside it.
    #[must_use]
    pub const fn orientation(&self) -> Versor {
        self.pose.rotation().to_versor()
    }
}
