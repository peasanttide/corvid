//! The surface: a partition of triangles, its seams, and its grid.

use alloc::vec::Vec;

use corvid_vector::{FinePoint, GlobalPoint};

use crate::cords::{NavCords, NavState, NavTriRef};
use crate::error::NavError;
use crate::grid::NavGrid;
use crate::seam::NavTriEdge;
use crate::step::Tune;
use crate::stitch::{exit_edge, seam_agrees, seam_map, slope_allows};
use crate::tri::NavTri;

/// How many triangles a walk may cross before it gives up and answers with
/// where it got to.
///
/// A wrong starting guess costs one hop per triangle between the guess and the
/// answer, and the grid's guess is never more than a cell away, so this is
/// slack rather than a budget.
const MAX_WALK: usize = 64;

/// A triangulated surface: the navmesh, the spatial index, the cold tier's
/// storage and the medium a rumour travels through, all at once.
///
/// The triangles are a **partition**, not a soup. Every point on the surface is
/// in exactly one of them, and where a query lands on a shared edge or vertex
/// the lower triangle index wins, so two peers asking the same question get the
/// same answer without agreeing on anything but the mesh.
///
/// Per-triangle payload -- what a crowd needs to know about the ground it is on
/// -- is **an index-parallel array the caller owns**, not a generic parameter.
/// [`tris`](Self::tris) hands out a slice whose indices are
/// [`NavTriRef`] values, so a caller keeps a `Vec<Whatever>` of the same length
/// beside it and indexes both with the same number. A generic would have put
/// the payload's type in the signature of every function here, in a crate that
/// is not allowed to know what a peasant is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavMesh {
    tris: Vec<NavTri>,
    grid: NavGrid,
}

impl NavMesh {
    /// Builds a mesh from ECEF vertices and the faces that index them.
    ///
    /// Adjacency comes from the indices: two faces are neighbours when they
    /// name the same pair of vertices, which is what makes the result a
    /// partition rather than a pile of triangles that happen to touch. The
    /// seam transforms and the walkability are computed once, here, because
    /// every crossing at play time then costs a multiply.
    ///
    /// # Errors
    ///
    /// [`NavError::VertexOutOfRange`] for a face naming a vertex that is not
    /// there, [`NavError::DegenerateFace`] for one with no area,
    /// [`NavError::EdgeTooLong`] for an edge past
    /// [`MAX_EDGE`](crate::MAX_EDGE), [`NavError::FaceTooSteep`] for a face
    /// whose local frame would be singular,
    /// [`NavError::NonManifoldEdge`] for an edge three faces share, and
    /// [`NavError::GridTooLarge`] for a mesh whose bounding box needs more grid
    /// than the grid is allowed to be.
    pub fn new(
        vertices: &[GlobalPoint],
        faces: &[[u32; 3]],
        tune: &Tune,
    ) -> Result<Self, NavError> {
        let mut tris = Vec::with_capacity(faces.len());
        for (face, corners) in faces.iter().enumerate() {
            let mut points = [GlobalPoint::ZERO; 3];
            for (slot, &corner) in corners.iter().enumerate() {
                let point = *vertices
                    .get(corner as usize)
                    .ok_or(NavError::VertexOutOfRange {
                        face,
                        vertex: corner,
                        count: vertices.len(),
                    })?;
                if let Some(place) = points.get_mut(slot) {
                    *place = point;
                }
            }
            tris.push(NavTri::build(face, points)?);
        }

        let mut mesh = Self {
            tris,
            grid: NavGrid::build(&[])?,
        };
        mesh.stitch(faces, tune)?;
        mesh.grid = NavGrid::build(&mesh.tris)?;
        Ok(mesh)
    }

    /// How many triangles the surface has.
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.tris.len()
    }

    /// Whether the surface has no triangles at all.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// Every triangle, in the order the faces were given.
    ///
    /// The index into this slice is the [`NavTriRef`], which is what makes an
    /// index-parallel payload array line up.
    #[must_use]
    #[inline]
    pub fn tris(&self) -> &[NavTri] {
        &self.tris
    }

    /// One triangle, or [`None`] if the reference is not one of this mesh's.
    #[must_use]
    #[inline]
    pub fn tri(&self, reference: NavTriRef) -> Option<&NavTri> {
        self.tris.get(reference.0 as usize)
    }

    /// The grid that guesses where a query starts.
    #[must_use]
    #[inline]
    pub const fn grid(&self) -> &NavGrid {
        &self.grid
    }

    /// The triangles across this one's three seams, in edge order.
    ///
    /// Edge order and nothing else: a diffusion that visited neighbours in a
    /// hash map's order would give two peers two different answers from the
    /// same field.
    pub fn neighbours(&self, reference: NavTriRef) -> impl Iterator<Item = NavTriRef> {
        self.tri(reference)
            .map_or([None; 3], NavTri::edges)
            .into_iter()
            .flatten()
            .map(NavTriEdge::next)
    }

    /// Whether the two sides of a seam agree about where the ground is.
    ///
    /// The derivation [physics.md] gives for walkability: carry the shared
    /// edge's two endpoints into the neighbour's frame and see whether they
    /// come back at the neighbour's own height, and inside it. Recomputed from
    /// the two triangles' frames rather than read off the stored seam, so that
    /// it is an independent check on what the builder wrote down.
    ///
    /// [`None`] if the reference or the edge names nothing.
    ///
    /// [physics.md]: https://github.com/peasanttide/peasanttide/blob/main/design/physics.md
    #[must_use]
    pub fn heights_agree(&self, reference: NavTriRef, edge: usize, tune: &Tune) -> Option<bool> {
        let from = self.tri(reference)?;
        let to = self.tri(from.edge(edge)?.next())?;
        Some(seam_agrees(seam_map(from, to), edge, tune))
    }

    /// Whether a body may cross a seam on foot.
    ///
    /// [`heights_agree`](Self::heights_agree), and then a slope test: a face
    /// steeper than [`Tune::max_slope`] is one a walker is turned back from
    /// even though the ground is continuous across the seam. The height check
    /// is what catches a mesh whose adjacency and geometry disagree; the slope
    /// check is what makes a cliff face a cliff.
    ///
    /// [`None`] if the reference or the edge names nothing.
    #[must_use]
    pub fn derive_walkable(&self, reference: NavTriRef, edge: usize, tune: &Tune) -> Option<bool> {
        let from = self.tri(reference)?;
        let to = self.tri(from.edge(edge)?.next())?;
        Some(seam_agrees(seam_map(from, to), edge, tune) && slope_allows(to, tune))
    }

    /// Which triangle is at a world position, and the coordinates there.
    ///
    /// The grid's cell gives a starting triangle and
    /// [`walk_toward`](Self::walk_toward) does the rest, so the answer does not
    /// depend on the guess being right. A point off the surface answers with
    /// the nearest coordinates on the triangle the walk ended in, because a
    /// [`NavCords`] has no way to say "nowhere".
    ///
    /// [`None`] only for an empty mesh.
    #[must_use]
    pub fn locate(&self, point: GlobalPoint) -> Option<NavCords> {
        let start = self
            .grid
            .lookup(point)
            .or_else(|| (!self.is_empty()).then_some(NavTriRef(0)))?;
        let found = self.walk_toward(start, point)?;
        let tri = self.tri(found)?;
        Some(NavCords::encode(NavState {
            tri: found,
            position: NavTri::clamp_inside(tri.local(point)),
            velocity: FinePoint::ZERO,
        }))
    }

    /// Walks from one triangle toward a world position, crossing seams.
    ///
    /// Each hop projects the target into the current triangle's frame and takes
    /// the edge a straight line from the centre leaves through, which is the
    /// same crossing arithmetic a step uses. Seams are crossed whether or not
    /// they are walkable: this is a spatial query, not a journey.
    ///
    /// The walk refuses to step straight back where it came from unless that is
    /// the only way out, which is what stops a concave fold -- where two
    /// triangles each think the target is behind the other -- from becoming a
    /// loop. A walk that runs out of hops answers with where it got to.
    #[must_use]
    pub fn walk_toward(&self, from: NavTriRef, target: GlobalPoint) -> Option<NavTriRef> {
        let mut current = from;
        let mut previous = None;
        for _ in 0..MAX_WALK {
            let tri = self.tri(current)?;
            let local = tri.local(target);
            if NavTri::contains(local) {
                return Some(current);
            }
            let Some(edge) = exit_edge(tri, local, previous) else {
                return Some(current);
            };
            let Some(seam) = tri.edge(edge) else {
                return Some(current);
            };
            previous = Some(current);
            current = seam.next();
        }
        Some(current)
    }

    /// Fills in every seam, once the triangles all have their frames.
    fn stitch(&mut self, faces: &[[u32; 3]], tune: &Tune) -> Result<(), NavError> {
        let mut seams: Vec<(u32, u32, u32, u8)> = Vec::with_capacity(faces.len() * 3);
        for (face, corners) in faces.iter().enumerate() {
            for edge in 0..3usize {
                let (Some(&from), Some(&to)) = (corners.get(edge), corners.get((edge + 1) % 3))
                else {
                    continue;
                };
                seams.push((
                    from.min(to),
                    from.max(to),
                    u32::try_from(face).unwrap_or(u32::MAX),
                    u8::try_from(edge).unwrap_or(0),
                ));
            }
        }
        // Sorted rather than hashed, and sorted on the whole tuple, so the
        // order two faces are stitched in is a fact about the mesh rather than
        // about the allocator.
        seams.sort_unstable();

        let mut start = 0;
        while let Some(&head) = seams.get(start) {
            let mut end = start + 1;
            while seams
                .get(end)
                .is_some_and(|next| next.0 == head.0 && next.1 == head.1)
            {
                end += 1;
            }
            match end - start {
                1 => {}
                2 => {
                    let (Some(&left), Some(&right)) = (seams.get(start), seams.get(start + 1))
                    else {
                        break;
                    };
                    self.link(left, right, tune);
                    self.link(right, left, tune);
                }
                _ => {
                    return Err(NavError::NonManifoldEdge {
                        from: head.0,
                        to: head.1,
                    });
                }
            }
            start = end;
        }
        Ok(())
    }

    /// Records the seam from one face's edge to another's.
    fn link(&mut self, source: (u32, u32, u32, u8), target: (u32, u32, u32, u8), tune: &Tune) {
        let (Some(&from), Some(&to)) = (
            self.tris.get(source.2 as usize),
            self.tris.get(target.2 as usize),
        ) else {
            return;
        };
        let edge = source.3 as usize;
        let map = seam_map(&from, &to);
        let walkable = seam_agrees(map, edge, tune) && slope_allows(&to, tune);
        if let Some(place) = self.tris.get_mut(source.2 as usize) {
            place.set_edge(edge, NavTriEdge::build(NavTriRef(target.2), walkable, map));
        }
    }
}
