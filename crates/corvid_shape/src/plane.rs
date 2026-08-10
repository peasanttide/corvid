//! An infinite flat surface, and the cast that finds it.

use corvid_fixed::I24F8;

use crate::{Cast, Hit, Ray};
use corvid_vector::{Direction, GlobalPoint};

/// An infinite plane: a normal, and how far along it the plane sits.
///
/// The ground, in a game whose ground is flat, and the near and far walls of
/// anything built out of half-spaces. `offset` is the signed distance from the
/// origin along `normal`, which is the form that makes
/// [`distance_to`](Self::distance_to) a subtraction rather than a projection of
/// a stored point.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct Plane {
    /// Which way is out.
    pub normal: Direction,
    /// How far from the origin along the normal the surface is.
    pub offset: I24F8,
}

impl Plane {
    /// A plane from its normal and its distance from the origin.
    #[must_use]
    #[inline]
    pub const fn new(normal: Direction, offset: I24F8) -> Self {
        Self { normal, offset }
    }

    /// The plane through a point, facing a direction.
    ///
    /// The spelling a game uses: *the ground is the plane through the origin
    /// facing up*.
    #[must_use]
    #[inline]
    pub fn through(point: GlobalPoint, normal: Direction) -> Self {
        Self::new(normal, point.project(normal))
    }

    /// How far a point is from the surface, signed: positive on the side the
    /// normal points to.
    #[must_use]
    #[inline]
    pub fn distance_to(&self, point: GlobalPoint) -> I24F8 {
        point.project(self.normal).saturating_sub(self.offset)
    }
}

/// One division, and two branches that between them are every way a ray can
/// fail to reach a plane.
///
/// ```text
/// denominator = normal * direction              Q31
/// numerator   = offset - normal * origin        Q8
/// t           = numerator / denominator         Q8, after a scaling by the unit
/// ```
///
/// A zero denominator is a ray parallel to the surface, which never arrives --
/// including the ray lying *in* the plane, which arrives everywhere and has no
/// single distance to report. A negative `t` is a plane behind the origin.
/// Neither is a division by zero and neither is a negative distance handed back
/// to a caller that will use it to place a cursor.
impl Cast for Plane {
    fn cast(&self, ray: Ray) -> Option<Hit> {
        let numerator = self.offset.saturating_sub(ray.origin.project(self.normal));
        // `None` for a zero denominator, which is the parallel ray this method
        // reports as a miss rather than dividing by.
        let distance = numerator.checked_div_signed32(self.normal.align(ray.direction))?;
        if distance.is_negative() {
            return None;
        }
        Some(Hit::new(ray, distance, self.normal))
    }
}
