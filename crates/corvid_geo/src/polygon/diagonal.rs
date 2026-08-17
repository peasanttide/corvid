//! Whether a segment between two boundary nodes stays inside the polygon.
//!
//! Three questions, and a diagonal has to answer all three. It must leave the
//! first node into the interior and arrive at the second from the interior --
//! that is [`locally_inside`], and it is a test on the two nodes' own corners.
//! It must not cross any edge -- that is [`crosses_boundary`]. And it must not
//! run through a hole, which the first two cannot see: a segment can leave a
//! convex corner, arrive at another, cross nothing, and still pass straight
//! through a courtyard. That is [`middle_inside`], which is an even-odd
//! containment test on the midpoint.

use crate::polygon::nodes::Nodes;
use crate::polygon::predicate::{cross, cross_properly};

/// Whether the segment from `a` toward `b` leaves `a` into the polygon.
///
/// The cone at `a` is bounded by its two edges, and which side of it the
/// interior is on depends on whether the corner is convex or reflex -- which
/// is why this is two tests rather than one.
pub(crate) fn locally_inside(nodes: &Nodes, a: u32, b: u32) -> bool {
    let corner = nodes.at(a);
    let before = nodes.at(nodes.prev(a));
    let after = nodes.at(nodes.next(a));
    let target = nodes.at(b);

    if cross(before, corner, after) > 0 {
        cross(corner, target, after) <= 0 && cross(corner, before, target) <= 0
    } else {
        cross(corner, target, before) > 0 || cross(corner, after, target) > 0
    }
}

/// Whether the segment `a` to `b` crosses any edge of the boundary.
///
/// `boundary` is the set of nodes whose outgoing edges make up the boundary --
/// one cycle while an ear is being clipped, every live node while a hole is
/// being bridged. Touching is not crossing, which matters because every
/// candidate diagonal shares its endpoints with four edges.
pub(crate) fn crosses_boundary(nodes: &Nodes, boundary: &[u32], a: u32, b: u32) -> bool {
    let (from, to) = (nodes.at(a), nodes.at(b));
    boundary
        .iter()
        .any(|&node| cross_properly(from, to, nodes.at(node), nodes.at(nodes.next(node))))
}

/// Whether the midpoint of `a` to `b` is inside the boundary, by even-odd.
///
/// The midpoint of two Q8 points is a half step, so every coordinate here is
/// doubled and the comparison against the crossing is cross-multiplied. Both
/// keep the test exact, which is the only way two machines agree on a diagonal
/// that grazes a vertex.
pub(crate) fn middle_inside(nodes: &Nodes, boundary: &[u32], a: u32, b: u32) -> bool {
    let (ax, ay) = nodes.at(a).bits();
    let (bx, by) = nodes.at(b).bits();
    let (mx, my) = (ax + bx, ay + by);

    let mut inside = false;
    for &node in boundary {
        let (px, py) = nodes.at(node).bits();
        let (qx, qy) = nodes.at(nodes.next(node)).bits();
        let (px, py, qx, qy) = (px * 2, py * 2, qx * 2, qy * 2);
        if (py > my) == (qy > my) {
            continue;
        }
        // `mx < px + (qx - px) (my - py) / (qy - py)`, multiplied out. The
        // divisor's sign is the inequality's direction, and it is never zero
        // because the two endpoints straddle the ray.
        let left = i128::from(mx - px) * i128::from(qy - py);
        let right = i128::from(qx - px) * i128::from(my - py);
        if (qy > py && left < right) || (qy < py && left > right) {
            inside = !inside;
        }
    }
    inside
}

/// Whether `a` to `b` is a diagonal: inside at both ends, crossing nothing,
/// and not passing through a hole.
pub(crate) fn is_diagonal(nodes: &Nodes, boundary: &[u32], a: u32, b: u32) -> bool {
    locally_inside(nodes, a, b)
        && locally_inside(nodes, b, a)
        && !crosses_boundary(nodes, boundary, a, b)
        && middle_inside(nodes, boundary, a, b)
}
