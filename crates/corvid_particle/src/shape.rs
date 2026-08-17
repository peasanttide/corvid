//! Where a particle is born and which way it leaves.

use corvid_float::consts::TAU;
use corvid_glm::Vec3;

use crate::Rng;
use crate::vector::{basis, normalized};

/// The offset from an emitter's position a particle is born at, and the
/// direction it leaves along.
///
/// One type for the two, because they are one decision: a shell throws its
/// particles outward from where it put them, and a ring that scattered its
/// particles in every direction would not be a ring for longer than a frame.
/// Speed is the emitter's, so a shape says which way and never how fast.
///
/// The four fires of the design each pick a different one. A wall burning is a
/// [`Cone`](Self::Cone) of smoke straight up, the core of a blast is a
/// [`Sphere`](Self::Sphere), a shockwave along the ground is a
/// [`Ring`](Self::Ring) with the world's up as its normal, and a room full of
/// fuel is a [`Volume`](Self::Volume).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Shape {
    /// Everything from one place, in every direction.
    #[default]
    Point,
    /// From inside a ball, outward.
    ///
    /// The offset is a uniform direction taken a uniform fraction of the way
    /// out, which is denser at the centre than a uniform ball would be -- and
    /// that is what a fireball looks like, so the cheaper sampling is also the
    /// better picture.
    Sphere {
        /// How far out a particle can be born.
        radius: f32,
    },
    /// From one place, within a half-angle of an axis.
    ///
    /// A `spread` of zero gives the axis exactly, which is the one shape a test
    /// can predict a particle's whole path from. Half a turn or more gives the
    /// sphere back.
    Cone {
        /// Which way the cone points. Normalized here; a zero axis is the
        /// world's up.
        axis: Vec3,
        /// The half-angle, in radians.
        spread: f32,
    },
    /// From a circle, outward along it, in the plane the normal defines.
    ///
    /// The shockwave: with the world's up as the normal the ring lies on the
    /// ground, every particle leaves along its own radius, and what expands is
    /// a circle rather than a cloud. Nothing here makes it expand -- that is
    /// the emitter's speed -- and nothing here keeps it in the plane either,
    /// because gravity is the emitter's too and a ring that wants to stay flat
    /// sets it to zero.
    Ring {
        /// The normal of the plane the ring lies in.
        normal: Vec3,
        /// How far from the emitter the circle is.
        radius: f32,
    },
    /// From inside a box, in every direction.
    ///
    /// The box is axis-aligned, because a particle emitter is not a transform
    /// hierarchy and a caller wanting one rotated has two emitters or a
    /// [`Cone`](Self::Cone).
    Volume {
        /// Half the box's size along each axis.
        half_extent: Vec3,
    },
}

impl Shape {
    /// Where a particle is born relative to the emitter, and which way it goes.
    ///
    /// A shape always takes the same number of draws from `rng` whatever its
    /// fields hold -- one for a [`Ring`](Self::Ring), two for a
    /// [`Point`](Self::Point) or a [`Cone`](Self::Cone), three for a
    /// [`Sphere`](Self::Sphere), five for a [`Volume`](Self::Volume) -- so that
    /// widening a radius in an editor moves the particles it should and
    /// renumbers nothing that comes after them.
    pub(crate) fn sample(self, rng: &mut Rng) -> (Vec3, Vec3) {
        match self {
            Self::Point => (Vec3::zeros(), rng.direction()),
            Self::Sphere { radius } => {
                let direction = rng.direction();
                (direction * (radius * rng.unit()), direction)
            }
            Self::Cone { axis, spread } => {
                let (right, up, forward) = basis(axis);
                // Uniform over the cap by Archimedes again: the height along
                // the axis is uniform between the rim and the pole, not the
                // angle, or the rim would be starved.
                let height = rng.range(corvid_float::cos(spread), 1.0);
                let turn = rng.range(0.0, TAU);
                let radius =
                    corvid_float::sqrt(corvid_float::clamp(1.0 - height * height, 0.0, 1.0));
                let direction = forward * height
                    + right * (radius * corvid_float::cos(turn))
                    + up * (radius * corvid_float::sin(turn));
                (Vec3::zeros(), normalized(direction, forward))
            }
            Self::Ring { normal, radius } => {
                let (right, up, _) = basis(normal);
                let turn = rng.range(0.0, TAU);
                let outward = right * corvid_float::cos(turn) + up * corvid_float::sin(turn);
                (outward * radius, outward)
            }
            Self::Volume { half_extent } => {
                let offset = Vec3::new(
                    half_extent.x * rng.range(-1.0, 1.0),
                    half_extent.y * rng.range(-1.0, 1.0),
                    half_extent.z * rng.range(-1.0, 1.0),
                );
                (offset, rng.direction())
            }
        }
    }
}
