//! What a hit against the ground, and a hit against a wall, leave behind.
//!
//! Split from [`step`](crate::kinematic_step) because none of it is about the
//! loop: each function here is a fact about one velocity meeting one plane.

use corvid_fixed::{Angle16, Factor16, I16F16};
use corvid_vector::{Direction, FinePoint};

use crate::cords::NavState;
use crate::step::Tune;
use crate::tri::{NavTri, fine_direction};

/// The velocity a hit against the triangle's own plane leaves behind.
pub(crate) fn resolve_ground(tri: &NavTri, velocity: FinePoint, tune: &Tune) -> FinePoint {
    let world = tri.local_to_ecef().apply(velocity);
    let normal = tri.normal();
    let approach = along(world, normal);
    if !approach.is_negative() {
        return velocity;
    }
    let kept = if slides(approach, world, tune.slide_angle) {
        Factor16::MIN
    } else {
        tune.restitution
    };
    tri.ecef_to_local()
        .apply(reflect(world, normal, approach, kept))
}

/// The velocity a hit against an edge the body may not cross leaves behind.
///
/// The wall's normal is a row of the triangle's ECEF-to-local matrix, which
/// costs nothing to have: row 0 is orthogonal to both the second edge vector
/// and the up direction, so it is exactly the outward normal of a vertical wall
/// standing on the edge those two span. Edge 0's is the two rows summed,
/// because its constraint is the two coordinates summed.
///
/// **The wall a body standing on the face meets is perpendicular to the face,
/// not to the world.** A vertical wall's normal leans out of a sloped face's
/// plane, so bouncing off it sends a walker up or into the ground, and the
/// ground collision that answers that sends it back into the wall: two events,
/// each undoing the other, neither taking any time, until the step's budget is
/// gone and the body has not moved. So the normal is taken back into the plane
/// of the face before the reflection, by [`in_plane`], and a body walking into
/// a wall slides along it however steep the ground under it is. A body in the
/// air keeps the same rule, because the wall it is about to hit is the one
/// standing on the face it is over.
pub(crate) fn resolve_wall(tri: &NavTri, state: NavState, edge: usize, tune: &Tune) -> FinePoint {
    let velocity = state.velocity;
    let [first, second, _] = tri.ecef_to_local().rows();
    let inward = match edge {
        1 => first,
        2 => second,
        _ => first.add(second).neg(),
    };
    let Some(upright) = inward.normalize() else {
        return velocity;
    };
    let normal = in_plane(upright, tri.normal()).unwrap_or(upright);

    let world = tri.local_to_ecef().apply(velocity);
    let approach = along(world, normal);
    if !approach.is_negative() {
        return velocity;
    }
    tri.ecef_to_local()
        .apply(reflect(world, normal, approach, tune.restitution))
}

/// A direction with its component along `face` taken out, renormalized.
///
/// [`None`] when nothing is left, which is a wall lying in the face -- a thing
/// no edge of a face this crate accepts can be, since a face steeper than
/// [`NavError::FaceTooSteep`]'s limit is refused outright.
#[inline]
fn in_plane(direction: Direction, face: Direction) -> Option<Direction> {
    let vector = fine_direction(direction);
    let leaning = along(vector, face);
    vector.sub(fine_direction(face).mul(leaning)).normalize()
}

/// `world` with its component along `normal` replaced by `-kept` of itself.
///
/// `kept` of zero is a slide and leaves the body moving along the surface;
/// anything more is a bounce.
#[inline]
fn reflect(world: FinePoint, normal: Direction, approach: I16F16, kept: Factor16) -> FinePoint {
    let damped = I16F16::from_bits(
        (i64::from(approach.to_bits()) * i64::from(kept.to_bits()) / 65_535) as i32,
    );
    let change = approach.saturating_add(damped).saturating_neg();
    world.add(fine_direction(normal).mul(change))
}

/// The component of a near-field vector along a unit direction.
#[inline]
fn along(vector: FinePoint, direction: Direction) -> I16F16 {
    let [x, y, z] = direction.to_array();
    let product = i128::from(vector.x().to_bits()) * i128::from(x.canonicalize().to_bits())
        + i128::from(vector.y().to_bits()) * i128::from(y.canonicalize().to_bits())
        + i128::from(vector.z().to_bits()) * i128::from(z.canonicalize().to_bits());
    let scale = i128::from(corvid_fixed::Signed32::MAX.to_bits());
    let rounded = if product >= 0 {
        (product + scale / 2) / scale
    } else {
        -((-product + scale / 2) / scale)
    };
    I16F16::saturating_from_bits(rounded as i64)
}

/// Whether a hit is shallow enough to slide.
///
/// `sin(incidence) = |approach| / |world|`, so the comparison is a pair of
/// squares and never an angle: no square root, no trigonometry, and the same
/// answer on every machine.
#[inline]
fn slides(approach: I16F16, world: FinePoint, limit: Angle16) -> bool {
    let sine = i128::from(limit.sin().to_bits());
    let normal = i128::from(approach.to_bits());
    let full = i128::from(corvid_fixed::Signed16::MAX.to_bits());
    normal * normal * full * full <= i128::from(world.length_squared()) * sine * sine
}
