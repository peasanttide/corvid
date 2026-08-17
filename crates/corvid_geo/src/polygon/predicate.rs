//! The integer predicates every polygon question here is decided by.
//!
//! All of them are exact. A [`GroundPoint`] is a pair of Q8 bit patterns, a
//! difference of two reaches `2^32`, and a product of two differences reaches
//! `2^64` -- so the accumulator is an `i128` and the sign of a cross product
//! is the true sign rather than a sign that survived rounding. That is the
//! whole reason a triangulation built on these answers the same triangles on
//! every machine.

use corvid_fixed::I48F16;

use crate::GroundPoint;
use crate::arith::round_div;

/// Twice the signed area of the triangle `a b c`, in Q16 square metres.
///
/// Positive when the three turn counterclockwise, zero when they are
/// collinear, negative when they turn clockwise.
pub(crate) fn cross(a: GroundPoint, b: GroundPoint, c: GroundPoint) -> i128 {
    let (ax, ay) = a.bits();
    let (bx, by) = b.bits();
    let (cx, cy) = c.bits();
    i128::from(bx - ax) * i128::from(cy - ay) - i128::from(by - ay) * i128::from(cx - ax)
}

/// Twice the signed area a closed ring encloses, in Q16 square metres.
///
/// Fanned from the first point rather than summed as `x_i y_{i+1} - x_{i+1}
/// y_i`, so every term is a product of two *differences* and stays small for a
/// polygon that sits far from the anchor. The two forms are algebraically
/// equal; this one keeps the accumulator away from its bound.
pub(crate) fn doubled_area(points: &[GroundPoint]) -> i128 {
    let Some((base, rest)) = points.split_first() else {
        return 0;
    };
    let mut doubled = 0;
    for pair in rest.windows(2) {
        if let [b, c] = pair {
            doubled += cross(*base, *b, *c);
        }
    }
    doubled
}

/// A doubled Q16 area as a signed area in square metres.
///
/// Saturates past `1.4e14` square metres, which is a quarter of the earth's
/// surface and not a level.
pub(crate) fn half_area(doubled: i128) -> I48F16 {
    I48F16::saturating_from_bits(round_div(doubled, 2))
}

/// Whether `p` lies in the triangle `a b c`, edges and corners included.
///
/// The triangle must be counterclockwise. Inclusive on purpose: an ear whose
/// boundary merely touches another vertex is not clipped, because clipping it
/// would emit a triangle that shares more than an edge with what is left.
pub(crate) fn in_triangle(a: GroundPoint, b: GroundPoint, c: GroundPoint, p: GroundPoint) -> bool {
    cross(a, b, p) >= 0 && cross(b, c, p) >= 0 && cross(c, a, p) >= 0
}

/// Whether `p` lies on the closed segment `a b`.
pub(crate) fn on_segment(a: GroundPoint, b: GroundPoint, p: GroundPoint) -> bool {
    if cross(a, b, p) != 0 {
        return false;
    }
    let (ax, ay) = a.bits();
    let (bx, by) = b.bits();
    let (px, py) = p.bits();
    px >= ax.min(bx) && px <= ax.max(bx) && py >= ay.min(by) && py <= ay.max(by)
}

/// Whether the segments `a b` and `c d` cross at a point interior to both.
///
/// Touching at an endpoint and running collinear are both *not* a crossing.
/// That is what makes this usable on a ring that has been bridged to a hole,
/// where the bridge is two coincident segments traversed in opposite
/// directions and every diagonal drawn near it touches something.
pub(crate) fn cross_properly(
    a: GroundPoint,
    b: GroundPoint,
    c: GroundPoint,
    d: GroundPoint,
) -> bool {
    let (from_a, from_b) = (cross(c, d, a), cross(c, d, b));
    let (from_c, from_d) = (cross(a, b, c), cross(a, b, d));
    if from_a == 0 || from_b == 0 || from_c == 0 || from_d == 0 {
        return false;
    }
    (from_a > 0) != (from_b > 0) && (from_c > 0) != (from_d > 0)
}

/// The winding number of a closed ring about `p`, doubled coordinates and all.
///
/// The ray is cast along `+x`, and the crossing rule is the half-open one --
/// an edge counts when its lower endpoint is at or below the ray and its upper
/// endpoint is strictly above -- so a vertex exactly on the ray is counted
/// once rather than twice or not at all.
pub(crate) fn winding_number(points: &[GroundPoint], p: GroundPoint) -> i32 {
    let (_, py) = p.bits();
    let mut winding = 0;
    for index in 0..points.len() {
        let Some(&a) = points.get(index) else {
            continue;
        };
        let Some(&b) = points.get((index + 1) % points.len()) else {
            continue;
        };
        let (_, ay) = a.bits();
        let (_, by) = b.bits();
        if ay <= py {
            if by > py && cross(a, b, p) > 0 {
                winding += 1;
            }
        } else if by <= py && cross(a, b, p) < 0 {
            winding -= 1;
        }
    }
    winding
}
