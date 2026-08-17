//! Ear clipping, with holes bridged in first.
//!
//! The order of operations is the whole algorithm. Every ring is pruned of the
//! vertices that turn through nothing, each hole is joined to the outer ring by
//! a diagonal so that one cycle is left, and then ears are clipped off that
//! cycle until three vertices remain. Nothing here is heuristic and nothing
//! here is floating point, so the triangles come out in one order on every
//! machine -- which is the requirement `corvid_nav` inherits when it indexes
//! them.
//!
//! Scanning for an ear costs a pass over the cycle and clipping costs a pass
//! per ear, so a ring of `n` points costs `O(n^2)`; bridging a hole costs a
//! scan of the boundary per candidate pair. This runs when a level is read,
//! never in a tick.

use alloc::vec::Vec;

use crate::GroundPoint;
use crate::polygon::diagonal::is_diagonal;
use crate::polygon::nodes::Nodes;
use crate::polygon::predicate::{cross, in_triangle};
use crate::polygon::{Ring, Triangulate, Triangulation};

/// Triangulates an outer ring with its holes.
pub(crate) fn triangulate(outer: &Ring, holes: &[Ring]) -> Result<Triangulation, Triangulate> {
    let mut points: Vec<GroundPoint> = Vec::new();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for ring in core::iter::once(outer).chain(holes) {
        let from = points.len();
        points.extend_from_slice(ring.points());
        ranges.push((from, points.len()));
    }

    let mut nodes = Nodes::new(points);
    let starts: Vec<Option<u32>> = ranges
        .iter()
        .map(|&(from, to)| {
            nodes
                .link(from, to)
                .and_then(|start| prune(&mut nodes, start))
        })
        .collect();

    // A hole that pruned away to nothing was never a hole. An outer ring that
    // did is a polygon with no area, and there is no honest triangulation of
    // one.
    let Some(Some(outer_start)) = starts.first().copied() else {
        return Err(Triangulate::Degenerate);
    };

    let mut boundary = outer_start;
    for &hole in starts.iter().skip(1).flatten() {
        boundary = bridge_hole(&mut nodes, boundary, hole)?;
    }

    let mut triangles = Vec::new();
    clip(&mut nodes, boundary, &mut triangles)?;
    Ok(Triangulation {
        points: nodes.points(),
        triangles,
    })
}

/// Drops the vertices of one cycle that turn through nothing, answering a
/// surviving node or `None` when fewer than three are left.
///
/// A vertex collinear with its neighbours is redundant, and a vertex that
/// doubles back along its own edge encloses no area; both leave the ring's
/// shape and its area exactly as they were, and removing them is what
/// guarantees every remaining vertex is strictly convex or strictly reflex.
/// The ear search depends on that: a vertex that is neither can never be
/// clipped and would stall the scan forever.
fn prune(nodes: &mut Nodes, start: u32) -> Option<u32> {
    let mut node = start;
    let mut remaining = nodes.cycle(start).len();
    let mut examined = 0;
    while remaining >= 3 && examined < remaining {
        let (before, after) = (nodes.prev(node), nodes.next(node));
        if nodes.at(node) == nodes.at(after)
            || cross(nodes.at(before), nodes.at(node), nodes.at(after)) == 0
        {
            nodes.unlink(node);
            remaining -= 1;
            node = before;
            examined = 0;
        } else {
            node = after;
            examined += 1;
        }
    }
    (remaining >= 3).then_some(node)
}

/// Joins one hole to the cycle it sits in, answering the merged cycle's start.
///
/// The search begins at the hole's easternmost vertex, which is the one most
/// likely to see the outer ring, and takes the first outer vertex that a
/// diagonal reaches. First rather than nearest: nearest would need a distance,
/// a distance needs a tie-break, and a tie-break by anything but index is a
/// choice that can differ between two builds of the same level.
fn bridge_hole(nodes: &mut Nodes, outer: u32, hole: u32) -> Result<u32, Triangulate> {
    let boundary = nodes.live();
    let outer_ring = nodes.cycle(outer);
    for inner in eastmost_first(nodes, hole) {
        for &anchor in &outer_ring {
            if is_diagonal(nodes, &boundary, inner, anchor) {
                nodes.bridge(anchor, inner);
                return Ok(anchor);
            }
        }
    }
    Err(Triangulate::Unbridged)
}

/// A hole's nodes, rotated so its easternmost vertex comes first.
fn eastmost_first(nodes: &Nodes, hole: u32) -> Vec<u32> {
    let ring = nodes.cycle(hole);
    let first = ring
        .iter()
        .enumerate()
        .max_by_key(|&(_, &node)| {
            let point = nodes.at(node);
            (point.east(), point.north(), core::cmp::Reverse(node))
        })
        .map_or(0, |(index, _)| index);
    ring.iter()
        .cycle()
        .skip(first)
        .take(ring.len())
        .copied()
        .collect()
}

/// Clips ears off one cycle until three vertices are left.
fn clip(nodes: &mut Nodes, start: u32, out: &mut Vec<[u32; 3]>) -> Result<(), Triangulate> {
    let mut node = start;
    let mut remaining = nodes.cycle(start).len();
    let mut stalled = 0;
    while remaining > 3 {
        let (before, after) = (nodes.prev(node), nodes.next(node));
        let turn = cross(nodes.at(before), nodes.at(node), nodes.at(after));
        if turn == 0 {
            // Bridging can leave a corner with no turn in it. It carries no
            // area, so it goes without a triangle.
            nodes.unlink(node);
            remaining -= 1;
            node = before;
            stalled = 0;
        } else if turn > 0 && is_ear(nodes, before, node, after, remaining) {
            out.push([nodes.point(before), nodes.point(node), nodes.point(after)]);
            nodes.unlink(node);
            remaining -= 1;
            node = after;
            stalled = 0;
        } else {
            node = after;
            stalled += 1;
            if stalled > remaining {
                return split(nodes, node, out);
            }
        }
    }

    if remaining == 3 {
        let (before, after) = (nodes.prev(node), nodes.next(node));
        out.push([nodes.point(before), nodes.point(node), nodes.point(after)]);
    }
    Ok(())
}

/// Whether the corner at `node` can be clipped off: convex already, and with
/// no reflex vertex in the way.
///
/// Only *reflex* vertices are in the way, and that is a fact about simple
/// polygons rather than an optimization. A convex vertex inside a candidate
/// ear could only have arrived there through an edge crossing the ear, and a
/// boundary that crosses itself was refused earlier. What makes the
/// distinction load bearing here is bridging: a bridge puts a second vertex at
/// a point that is already on the boundary, and a rule that let any coincident
/// vertex block an ear would refuse every ear near the bridge and stall on the
/// first courtyard it met.
///
/// The containment test itself includes the triangle's edges, so a reflex
/// vertex merely touching the ear still refuses it -- clipping then would
/// leave two triangles sharing more than an edge, which is a triangulation
/// `corvid_nav` cannot walk across.
fn is_ear(nodes: &Nodes, before: u32, node: u32, after: u32, bound: usize) -> bool {
    let (a, b, c) = (nodes.at(before), nodes.at(node), nodes.at(after));
    let mut probe = nodes.next(after);
    for _ in 0..bound {
        if probe == before {
            return true;
        }
        let corner = nodes.at(probe);
        let reflex = cross(
            nodes.at(nodes.prev(probe)),
            corner,
            nodes.at(nodes.next(probe)),
        ) <= 0;
        if reflex && in_triangle(a, b, c, corner) {
            return false;
        }
        probe = nodes.next(probe);
    }
    true
}

/// Cuts a stalled cycle in two along a diagonal and clips each half.
///
/// A simple polygon always has an ear, so reaching here means the cycle is
/// only weakly simple -- which the bridges themselves make it, since each one
/// puts two coincident vertices on the boundary. Splitting is the way out that
/// keeps the partition exact; failing to find any diagonal at all means the
/// boundary crosses itself, and there is no partition to find.
fn split(nodes: &mut Nodes, start: u32, out: &mut Vec<[u32; 3]>) -> Result<(), Triangulate> {
    let ring = nodes.cycle(start);
    for (index, &a) in ring.iter().enumerate() {
        for &b in ring.iter().skip(index + 2) {
            if nodes.next(b) == a || nodes.point(a) == nodes.point(b) {
                continue;
            }
            if is_diagonal(nodes, &ring, a, b) {
                let other = nodes.split(a, b);
                clip(nodes, a, out)?;
                return clip(nodes, other, out);
            }
        }
    }
    Err(Triangulate::NotSimple)
}
