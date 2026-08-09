//! An infinite flat surface, and the cast that finds it.

use corvid_fixed::I24F8;

use crate::{
    Cast, Hit, Ray, align,
    project::{UNIT, divide, narrow, project},
};
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
        Self::new(normal, project(point, normal))
    }

    /// How far a point is from the surface, signed: positive on the side the
    /// normal points to.
    #[must_use]
    #[inline]
    pub fn distance_to(&self, point: GlobalPoint) -> I24F8 {
        project(point, self.normal).saturating_sub(self.offset)
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
        let denominator = i128::from(align(self.normal, ray.direction).to_bits());
        if denominator == 0 {
            return None;
        }
        let numerator = i128::from(
            self.offset
                .saturating_sub(project(ray.origin, self.normal))
                .to_bits(),
        );
        // Multiplying by the unit is what turns a Q8 over a Q31 back into a
        // Q8 -- the unit rather than a shift of 31, for the reason `project`'s
        // own module documents.
        let distance = divide(numerator * UNIT, denominator);
        if distance < 0 {
            return None;
        }
        Some(Hit::new(
            ray,
            I24F8::from_bits(narrow(distance)),
            self.normal,
        ))
    }
}
