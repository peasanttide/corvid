//! Three points, and the cast that picks a face out of a mesh.

use corvid_fixed::I24F8;

use crate::{
    Aabb, Cast, Hit, Ray,
    project::{UNIT, cross_wide, divide, narrow},
};
use corvid_vector::{Direction, GlobalPoint};

/// A triangle, wound in the order given.
///
/// The unit a mesh pick works in. A game that lets a player point at its
/// geometry casts at these; a game whose world is a cell graph — the example
/// game's is — casts at a [`Sphere`](crate::Sphere) and looks the cell up, and
/// never needs this at all.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct Triangle {
    /// The first corner.
    pub a: GlobalPoint,
    /// The second.
    pub b: GlobalPoint,
    /// The third.
    pub c: GlobalPoint,
}

impl Triangle {
    /// A triangle from three corners.
    #[must_use]
    #[inline]
    pub const fn new(a: GlobalPoint, b: GlobalPoint, c: GlobalPoint) -> Self {
        Self { a, b, c }
    }

    /// Which way the face points, by the right-hand rule on the winding given.
    ///
    /// [`None`] for a degenerate triangle — three collinear points, or two that
    /// coincide — which has no plane and therefore no normal. That is also the
    /// case [`cast`](Cast::cast) reports as a miss.
    #[must_use]
    pub fn normal(&self) -> Option<Direction> {
        (self.b - self.a).cross(self.c - self.a).normalize()
    }

    /// The smallest axis-aligned box holding all three corners.
    #[must_use]
    #[inline]
    pub fn bounds(&self) -> Aabb {
        Aabb::from_points([self.a, self.b, self.c])
    }
}

/// Möller–Trumbore, with every intermediate in `i128` and no narrowing until
/// the distance is handed back.
///
/// The routine finds the barycentric coordinates and the distance in one pass,
/// and it is written here as three wide dot products against two wide cross
/// products:
///
/// ```text
/// e1 = b − a                      Q8
/// e2 = c − a                      Q8
/// p  = direction × e2             Q39
/// det = e1 · p                    Q47
/// s  = origin − a                 Q8
/// u  = (s · p) ÷ det              dimensionless
/// q  = s × e1                     Q16
/// v  = (direction · q) ÷ det      dimensionless
/// t  = (e2 · q) ÷ det             Q−23, scaled by the unit to reach Q8
/// ```
///
/// Two things about it are decisions rather than transcription.
///
/// **The barycentric tests do not divide.** `u` and `v` are compared against
/// zero and against `det` with the sign of `det` folded in, rather than being
/// divided out and compared against zero and one. A division that rounded would
/// let a ray through the seam between two triangles that share an edge, which
/// is a hole in a mesh that appears at one pixel and is never reproducible.
///
/// **There is no back-face culling.** A cast at the inside of a planet's shell
/// is a legitimate hit, and this crate does not decide otherwise. A caller that
/// wants culling compares [`align`](crate::align) of the normal against the
/// ray's direction, which is one line and is why that function is public.
impl Cast for Triangle {
    fn cast(&self, ray: Ray) -> Option<Hit> {
        let first = self.b - self.a;
        let second = self.c - self.a;

        let pitch = cross_wide(direction_bits(ray), point_bits(second));
        let determinant = dot_wide(point_bits(first), pitch);
        if determinant == 0 {
            return None;
        }

        let to_origin = ray.origin - self.a;
        let u = dot_wide(point_bits(to_origin), pitch);
        if outside(u, determinant) {
            return None;
        }

        let across = cross_wide(point_bits(to_origin), point_bits(first));
        let v = dot_wide(direction_bits(ray), across);
        if outside(v, determinant) || outside(u + v, determinant) {
            return None;
        }

        // The determinant carries one factor of the direction's unit, which the
        // numerator does not — so multiplying by the unit is what brings the
        // ratio back to a Q8. It can overflow only for geometry spanning most
        // of the world, which this reports as a miss rather than as a wrapped
        // distance.
        let scaled = dot_wide(point_bits(second), across).checked_mul(UNIT)?;
        let distance = divide(scaled, determinant);
        if distance < 0 {
            return None;
        }

        Some(Hit::new(
            ray,
            I24F8::from_bits(narrow(distance)),
            self.normal()?,
        ))
    }
}

/// Whether a barycentric numerator falls outside `0 ..= determinant`.
///
/// Written with the determinant's sign folded in rather than divided out, for
/// the reason the routine's own documentation gives.
#[must_use]
#[inline]
const fn outside(numerator: i128, determinant: i128) -> bool {
    if determinant > 0 {
        numerator < 0 || numerator > determinant
    } else {
        numerator > 0 || numerator < determinant
    }
}

/// A point's three components as wide integers, at whatever scale it holds.
#[must_use]
#[inline]
fn point_bits(point: GlobalPoint) -> [i128; 3] {
    point.to_array().map(|value| i128::from(value.to_bits()))
}

/// A ray's direction as wide integers.
#[must_use]
#[inline]
fn direction_bits(ray: Ray) -> [i128; 3] {
    ray.direction
        .to_array()
        .map(|value| i128::from(value.to_bits()))
}

/// A dot product of two wide triples, at the sum of their scales.
#[must_use]
#[inline]
const fn dot_wide(a: [i128; 3], b: [i128; 3]) -> i128 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
