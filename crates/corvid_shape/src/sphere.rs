//! A ball, and the cast that finds it.

use corvid_fixed::I24F8;

use crate::{Cast, Hit, Ray};
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
        // Squared on both sides, so there is no square root and no floating
        // point. A point further from the centre than an `I24F8` reaches
        // answers the largest squared distance there is, which is above every
        // radius -- so the comparison is still an answer out there.
        point.distance_squared(self.centre) <= self.radius.squared()
    }
}

/// The quadratic, worked at the doubled scale so that its square root lands
/// back at the ordinary one with no further scaling.
///
/// With `oc` the offset from the centre to the ray's origin:
///
/// ```text
/// along = oc . direction                     Q8
/// miss  = |oc|^2 - along^2                   Q16, the squared closest approach
/// disc  = radius^2 - miss                    Q16
/// root  = sqrt(disc)                         Q8, because sqrt(Q16) is Q8
/// t     = -along -/+ root
/// ```
///
/// Choosing the doubled scale as the working one is the whole trick: an
/// integer square root of a Q16 value *is* the Q8 answer, so there is no
/// scaling step to get wrong and no floating point anywhere.
///
/// `miss` is asked for directly rather than assembled from its two terms, which
/// is what keeps a cast at the far edge of the range honest. Both `|oc|^2` and
/// `along^2` reach `2^62` there and their difference is small, so subtracting
/// them here would be subtracting two large numbers to get a small one -- and
/// the small one is what decides hit from miss.
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
        // direction the `along` term is zero, so an origin *inside* the sphere
        // leaves a positive discriminant and the quadratic reports a hit at a
        // distance the ray never travels.

        // A sphere further from the origin than an `I24F8` reaches has no hit
        // distance this could report, so it is a miss rather than a clamp --
        // and `checked_sub` rather than the saturating one is what makes that
        // the answer instead of a hit at a place the ray does not pass.
        let offset = ray.origin.checked_sub(self.centre)?;
        let discriminant = self
            .radius
            .squared()
            .saturating_sub(offset.rejection_squared(ray.direction));
        if discriminant.is_negative() {
            return None;
        }
        let root = discriminant.root();

        let centred = offset.project(ray.direction).saturating_neg();
        let near = centred.saturating_sub(root);
        let far = centred.saturating_add(root);
        let distance = if !near.is_negative() {
            near
        } else if !far.is_negative() {
            far
        } else {
            return None;
        };

        let normal = (ray.at(distance) - self.centre).normalize()?;
        Some(Hit::new(ray, distance, normal))
    }
}
