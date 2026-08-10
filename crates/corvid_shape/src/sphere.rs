//! A ball, and the cast that finds it.

use corvid_fixed::I24F8;

use crate::{
    Cast, Hit, Ray,
    project::{length_squared_bits, narrow, offset_bits, project_bits},
};
use corvid_vector::GlobalPoint;

/// A ball: a centre and a radius.
///
/// The bounding volume for almost everything, and the shape a planet is when a
/// swarm player is holding it at arm's length.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct Sphere {
    /// The middle.
    pub centre: GlobalPoint,
    /// How far the surface is from it. A negative radius is empty rather than
    /// inside out: nothing here squares it and hopes.
    pub radius: I24F8,
}

impl Sphere {
    /// A ball.
    #[must_use]
    #[inline]
    pub const fn new(centre: GlobalPoint, radius: I24F8) -> Self {
        Self { centre, radius }
    }

    /// Whether a point is inside, boundary included.
    ///
    /// The boundary is inside for the reason [`Aabb::contains`](crate::Aabb)'s
    /// is: a half-open surface makes a point on the skin of two touching balls
    /// belong to neither, which is a hole in whatever index is being built out
    /// of them.
    #[must_use]
    pub fn contains(&self, point: GlobalPoint) -> bool {
        if self.radius.is_negative() {
            return false;
        }
        let radius = i128::from(self.radius.to_bits());
        length_squared_bits(offset_bits(point, self.centre)) <= radius * radius
    }
}

/// The quadratic, worked at the doubled scale so that its square root lands
/// back at the ordinary one with no further scaling.
///
/// With `oc` the offset from the centre to the ray's origin:
///
/// ```text
/// b    = oc * direction                 Q8
/// c    = |oc|^2 - radius^2                Q16
/// disc = b^2 - c                         Q16
/// root = sqrt(disc)                          Q8, because sqrt(Q16) is Q8
/// t    = -b -/+ root
/// ```
///
/// Choosing the doubled scale as the working one is the whole trick: an
/// integer square root of a Q16 value *is* the Q8 answer, so there is no
/// scaling step to get wrong and no floating point anywhere.
///
/// The near root is taken when it is in front of the origin and the far one
/// when it is not, which is what makes a ray that starts inside answer the wall
/// it leaves through rather than a negative distance. Both behind is a miss.
impl Cast for Sphere {
    fn cast(&self, ray: Ray) -> Option<Hit> {
        if self.radius.is_negative() || ray.is_degenerate() {
            return None;
        }
        // A degenerate ray is caught above rather than falling through: with no
        // direction the `b` term is zero, so an origin *inside* the sphere
        // leaves a positive discriminant and the quadratic reports a hit at a
        // distance the ray never travels.
        let offset = offset_bits(ray.origin, self.centre);
        let b = project_bits(offset, ray.direction);
        let radius = i128::from(self.radius.to_bits());
        let c = length_squared_bits(offset) - radius * radius;

        let discriminant = b * b - c;
        if discriminant < 0 {
            return None;
        }
        let root = discriminant.isqrt();

        let near = -b - root;
        let far = -b + root;
        let distance = if near >= 0 {
            near
        } else if far >= 0 {
            far
        } else {
            return None;
        };

        let distance = I24F8::from_bits(narrow(distance));
        let normal = (ray.at(distance) - self.centre).normalize()?;
        Some(Hit::new(ray, distance, normal))
    }
}
