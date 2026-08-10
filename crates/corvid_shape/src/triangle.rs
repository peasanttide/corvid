//! Three points, and the cast that picks a face out of a mesh.

use crate::{Aabb, Cast, Hit, Ray};
use corvid_vector::{Direction, GlobalPoint};

/// A triangle, wound in the order given.
///
/// The unit a mesh pick works in. A game that lets a player point at its
/// geometry casts at these; a game whose world is a cell graph -- the example
/// game's is -- casts at a [`Sphere`](crate::Sphere) and looks the cell up, and
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
    /// [`None`] for a degenerate triangle -- three collinear points, or two that
    /// coincide -- which has no plane and therefore no normal. That is also the
    /// case [`cast`](Cast::cast) reports as a miss.
    #[must_use]
    pub fn normal(&self) -> Option<Direction> {
        // `cross_direction` rather than `cross().normalize()`: the cross of two
        // edges reaches `2^62` and the narrower one divides it back into a
        // component's range first, which does not lose precision so much as
        // answer a different direction.
        let first = self.b.checked_sub(self.a)?;
        let second = self.c.checked_sub(self.a)?;
        first.cross_direction(second)
    }

    /// The smallest axis-aligned box holding all three corners.
    #[must_use]
    #[inline]
    pub fn bounds(&self) -> Aabb {
        Aabb::from_points([self.a, self.b, self.c])
    }
}

/// Moller-Trumbore, written as the four signed volumes it actually is.
///
/// The routine finds the barycentric coordinates and the distance in one pass,
/// and every quantity in it is a scalar triple product -- the volume of the box
/// three of the edges span. Writing it that way is what keeps the widening out
/// of this crate: a volume of two world-spanning edges and a direction has no
/// fixed-point type to be, so [`Volume`](corvid_vector::Volume) is opaque and
/// the only things asked of it are the ones a barycentric test needs.
///
/// ```text
/// e1  = b - a
/// e2  = c - a
/// s   = origin - a
/// det = e1 . (direction x e2)
/// u   = s  . (direction x e2)     inside when 0 <= u <= det
/// v   = e1 . (direction x s)      inside when 0 <= v <= det
/// hit = a + e1 (u/det) + e2 (v/det)
/// t   = (hit - origin) . direction
/// ```
///
/// Three things about it are decisions rather than transcription.
///
/// **The barycentric tests do not divide.** `u` and `v` are compared against
/// zero and against `det` with the sign of `det` folded in, rather than being
/// divided out and compared against zero and one. A division that rounded would
/// let a ray through the seam between two triangles that share an edge, which
/// is a hole in a mesh that appears at one pixel and is never reproducible.
///
/// **There is no back-face culling.** A cast at the inside of a planet's shell
/// is a legitimate hit, and this crate does not decide otherwise. A caller that
/// wants culling compares [`align`](corvid_vector::Direction::align) of the
/// normal against the ray's direction, which is one line.
///
/// **The distance comes from the point rather than a fourth determinant.** The
/// textbook fourth is `e2 . (s x e1)` over the same `det`, and it is a volume
/// where the other three are areas -- a whole metre of scale further out, which
/// is what used to make this the one place in the workspace needing an
/// accumulator past a word. Once `u` and `v` are known the hit is located
/// already, so walking to it and projecting back onto the ray answers the same
/// distance out of quantities that are all in range.
impl Cast for Triangle {
    fn cast(&self, ray: Ray) -> Option<Hit> {
        // `checked_sub` throughout: an edge or a ray origin further from the
        // first corner than an `I24F8` reaches has no offset in this type, and
        // a saturated one would run the arithmetic below against a triangle
        // that is not this one. A miss is the honest answer.
        let first = self.b.checked_sub(self.a)?;
        let second = self.c.checked_sub(self.a)?;
        let to_origin = ray.origin.checked_sub(self.a)?;

        let determinant = first.volume(ray.direction, second);
        if determinant.is_zero() {
            return None;
        }

        let u = to_origin.volume(ray.direction, second);
        if u.is_outside(determinant) {
            return None;
        }

        let v = first.volume(ray.direction, to_origin);
        if v.is_outside(determinant) || u.add(v).is_outside(determinant) {
            return None;
        }

        // Inside, so the two ratios are weights along the two edges and the
        // hit is where they put it. This is the only division in the routine
        // and it happens after every decision has been made, which is the
        // point of the comparisons above.
        let hit = self.a.lerp(self.b, u.ratio(determinant))
            + GlobalPoint::ZERO.lerp(second, v.ratio(determinant));
        let distance = hit.checked_sub(ray.origin)?.project(ray.direction);
        if distance.is_negative() {
            return None;
        }

        Some(Hit::new(ray, distance, self.normal()?))
    }
}
