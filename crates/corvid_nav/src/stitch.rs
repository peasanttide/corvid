//! What the builder works out about a seam, and what a query works out about an
//! edge.
//!
//! Split from [`NavMesh`](crate::NavMesh) because none of it is about the mesh:
//! every function here takes the two triangles, or the one, that it is a fact
//! about.

use corvid_fixed::I16F16;
use corvid_vector::FinePoint;

use crate::cords::NavTriRef;
use crate::linear::Affine3;
use crate::step::Tune;
use crate::tri::{NavTri, fine_offset};

/// How far a carried seam endpoint may sit outside the neighbour and still
/// count as landing in it.
///
/// One part in a thousand of a triangle, which is thirty times finer than a
/// position code and far coarser than the last bit of the matrices, so it
/// answers rounding without answering geometry.
const SEAM_TOLERANCE: I16F16 = I16F16::from_bits(64);

/// The centre of a triangle in its own local frame.
pub(crate) const CENTROID: FinePoint = FinePoint::new(
    I16F16::from_bits(21_845),
    I16F16::from_bits(21_845),
    I16F16::ZERO,
);

/// The map from one triangle's local frame into a neighbour's.
///
/// Composed rather than stored twice: `to`'s ECEF-to-local applied after
/// `from`'s local-to-ECEF, with the difference between the two origins carried
/// through as the translation.
pub(crate) fn seam_map(from: &NavTri, to: &NavTri) -> Affine3 {
    Affine3::new(
        to.ecef_to_local().compose(from.local_to_ecef()),
        to.ecef_to_local()
            .apply(fine_offset(from.origin(), to.origin())),
    )
}

/// The two endpoints of an edge, in the local frame of the triangle it belongs
/// to.
///
/// Vertex 0 is `(1, 0)`, vertex 1 is `(0, 1)` and vertex 2 is the origin, so an
/// edge's endpoints are two of those three and no arithmetic is needed to find
/// them.
const fn edge_ends(edge: usize) -> [FinePoint; 2] {
    let first = FinePoint::new(I16F16::ONE, I16F16::ZERO, I16F16::ZERO);
    let second = FinePoint::new(I16F16::ZERO, I16F16::ONE, I16F16::ZERO);
    let third = FinePoint::ZERO;
    match edge {
        0 => [first, second],
        1 => [second, third],
        _ => [third, first],
    }
}

/// Whether a seam's two endpoints land on the neighbour's own surface.
pub(crate) fn seam_agrees(map: Affine3, edge: usize, tune: &Tune) -> bool {
    edge_ends(edge).into_iter().all(|end| {
        let there = map.apply(end);
        there.z().abs() <= tune.step_height
            && there.x() >= SEAM_TOLERANCE.saturating_neg()
            && there.y() >= SEAM_TOLERANCE.saturating_neg()
            && there.x().saturating_add(there.y()) <= I16F16::ONE.saturating_add(SEAM_TOLERANCE)
    })
}

/// Whether a face is shallow enough for [`Tune::max_slope`].
pub(crate) fn slope_allows(tri: &NavTri, tune: &Tune) -> bool {
    tri.normal().align(tri.down().neg()).to_signed16() >= tune.max_slope.cos()
}

/// The edge a straight line from the centre toward `local` leaves through.
///
/// The three edges are three linear inequalities, so each candidate is one
/// division, and the earliest crossing wins with a tie going to the lower edge
/// index. An edge leading back to `previous` is taken only when nothing else
/// will do.
pub(crate) fn exit_edge(
    tri: &NavTri,
    local: FinePoint,
    previous: Option<NavTriRef>,
) -> Option<usize> {
    let heading = local.sub(CENTROID);
    let one = I16F16::ONE;
    let boundaries = [
        (
            one.saturating_sub(CENTROID.x())
                .saturating_sub(CENTROID.y()),
            heading.x().saturating_add(heading.y()).saturating_neg(),
        ),
        (CENTROID.x(), heading.x()),
        (CENTROID.y(), heading.y()),
    ];

    let mut best: Option<(I16F16, usize)> = None;
    let mut fallback: Option<(I16F16, usize)> = None;
    for (edge, &(distance, rate)) in boundaries.iter().enumerate() {
        if !rate.is_negative() || distance.is_negative() {
            continue;
        }
        let when = distance.saturating_div(rate.saturating_neg());
        let Some(seam) = tri.edge(edge) else {
            continue;
        };
        if fallback.is_none_or(|(held, _)| when < held) {
            fallback = Some((when, edge));
        }
        if Some(seam.next()) != previous && best.is_none_or(|(held, _)| when < held) {
            best = Some((when, edge));
        }
    }
    best.or(fallback).map(|(_, edge)| edge)
}
