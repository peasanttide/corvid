//! The eight-metre grid that guesses where a query starts.

use alloc::vec;
use alloc::vec::Vec;

use corvid_fixed::I24F8;
use corvid_vector::GlobalPoint;

use crate::cords::NavTriRef;
use crate::error::NavError;
use crate::tri::NavTri;

/// How far to shift an [`I24F8`] bit pattern to get a cell coordinate.
///
/// Eight metres at 1/256 of a metre is 2048 bit patterns, so the division is a
/// shift and an arithmetic shift floors, which is what a cell coordinate wants
/// on both sides of zero.
const CELL_SHIFT: u32 = 11;

/// The most cells a grid may hold, which is four bytes each.
///
/// A surface is a thin thing in a three-dimensional grid, so most of these are
/// empty and the cap is really a cap on the level's bounding box: about 33 km
/// on a side at the eight-metre resolution, or a smaller box in every
/// direction. A world larger than that is streamed as several meshes.
const MAX_CELLS: u64 = 1 << 24;

/// The code for a cell no triangle reached.
const EMPTY: u32 = u32::MAX;

/// How far out from an empty cell a lookup will look.
const SEARCH_RINGS: i64 = 3;

/// Where to start looking for the triangle under a point.
///
/// Each cell holds the triangle covering most of it, and a query starts there
/// and walks toward its target with the same crossing algorithm a step uses.
/// The grid is therefore allowed to be wrong: it is a guess that saves the walk
/// most of its hops, and `tests/fold.rs` starts a walk from a deliberately bad
/// one to prove the answer does not depend on it.
///
/// "Most of it" is measured by sampling: a triangle's three vertices, three
/// edge midpoints and centroid are each assigned to a cell, and the cell keeps
/// whichever triangle put the most samples in it, ties going to the lower
/// index. A triangle is at most as wide as a cell, so the seven samples settle
/// it in every case that is not already a tie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavGrid {
    origin: [i32; 3],
    dims: [u32; 3],
    cells: Vec<u32>,
}

impl NavGrid {
    /// The cell size in metres.
    pub const CELL: I24F8 = I24F8::from_bits(1 << CELL_SHIFT);

    /// How many cells across the grid is, in each axis.
    #[must_use]
    #[inline]
    pub const fn dims(&self) -> [u32; 3] {
        self.dims
    }

    /// The triangle to start a query for `point` from, or [`None`] if no cell
    /// within three of it holds one.
    #[must_use]
    pub fn lookup(&self, point: GlobalPoint) -> Option<NavTriRef> {
        let home = self.cell_of(point);
        let mut ring = 0;
        while ring <= SEARCH_RINGS {
            for z in -ring..=ring {
                for y in -ring..=ring {
                    for x in -ring..=ring {
                        if x.abs().max(y.abs()).max(z.abs()) != ring {
                            continue;
                        }
                        if let Some(found) = self.at([home[0] + x, home[1] + y, home[2] + z]) {
                            return Some(found);
                        }
                    }
                }
            }
            ring += 1;
        }
        None
    }

    /// Which cell a world position falls in, unclamped, so that a point outside
    /// the grid keeps its direction rather than folding onto an edge.
    fn cell_of(&self, point: GlobalPoint) -> [i64; 3] {
        let [x, y, z] = point.to_array();
        [
            (i64::from(x.to_bits()) - i64::from(self.origin[0])) >> CELL_SHIFT,
            (i64::from(y.to_bits()) - i64::from(self.origin[1])) >> CELL_SHIFT,
            (i64::from(z.to_bits()) - i64::from(self.origin[2])) >> CELL_SHIFT,
        ]
    }

    /// What a cell holds, or [`None`] if it is outside the grid or empty.
    fn at(&self, cell: [i64; 3]) -> Option<NavTriRef> {
        let index = self.index_of(cell)?;
        match self.cells.get(index) {
            Some(&EMPTY) | None => None,
            Some(&found) => Some(NavTriRef(found)),
        }
    }

    /// A cell's offset into the flat array, or [`None`] if it is outside.
    fn index_of(&self, cell: [i64; 3]) -> Option<usize> {
        let mut index = 0u64;
        let mut axis = 2;
        loop {
            let bound = i64::from(self.dims[axis]);
            if cell[axis] < 0 || cell[axis] >= bound {
                return None;
            }
            index = index * u64::try_from(bound).ok()? + u64::try_from(cell[axis]).ok()?;
            if axis == 0 {
                break;
            }
            axis -= 1;
        }
        usize::try_from(index).ok()
    }

    /// Builds the grid over a mesh's triangles.
    ///
    /// # Errors
    ///
    /// [`NavError::GridTooLarge`] when the mesh's bounding box needs more cells
    /// to cover than the grid is allowed to hold.
    pub(crate) fn build(tris: &[NavTri]) -> Result<Self, NavError> {
        let Some(first) = tris.first() else {
            return Ok(Self {
                origin: [0; 3],
                dims: [0; 3],
                cells: Vec::new(),
            });
        };

        let mut low = first.triangle()[0];
        let mut high = low;
        for tri in tris {
            for vertex in tri.triangle() {
                low = low.min(vertex);
                high = high.max(vertex);
            }
        }

        let origin = [
            align_down(low.x()),
            align_down(low.y()),
            align_down(low.z()),
        ];
        let mut dims = [0u32; 3];
        let mut cells = 1u64;
        for (axis, span) in high.to_array().iter().enumerate() {
            let reach = (i64::from(span.to_bits()) - i64::from(origin[axis])) >> CELL_SHIFT;
            let across = u32::try_from(reach + 1).unwrap_or(u32::MAX);
            dims[axis] = across;
            cells *= u64::from(across);
            if cells > MAX_CELLS {
                return Err(NavError::GridTooLarge {
                    cells,
                    limit: MAX_CELLS,
                });
            }
        }

        let mut grid = Self {
            origin,
            dims,
            cells: vec![EMPTY; usize::try_from(cells).unwrap_or(0)],
        };
        let mut best = vec![0u8; grid.cells.len()];
        for (index, tri) in tris.iter().enumerate() {
            grid.claim(u32::try_from(index).unwrap_or(EMPTY), tri, &mut best);
        }
        Ok(grid)
    }

    /// Records one triangle's samples against the cells they land in.
    fn claim(&mut self, index: u32, tri: &NavTri, best: &mut [u8]) {
        let samples = coverage_samples(tri);
        for (position, sample) in samples.iter().enumerate() {
            let cell = self.cell_of(*sample);
            // Only the first sample in a cell scores it, so a cell reached
            // twice by one triangle is counted once with the full tally.
            if samples[..position]
                .iter()
                .any(|earlier| self.cell_of(*earlier) == cell)
            {
                continue;
            }
            let score = samples
                .iter()
                .filter(|other| self.cell_of(**other) == cell)
                .count();
            let Some(offset) = self.index_of(cell) else {
                continue;
            };
            let (Some(held), Some(current)) = (best.get(offset), self.cells.get(offset)) else {
                continue;
            };
            let score = u8::try_from(score).unwrap_or(u8::MAX);
            if (score > *held || (score == *held && index < *current))
                && let (Some(slot), Some(mark)) = (self.cells.get_mut(offset), best.get_mut(offset))
            {
                *slot = index;
                *mark = score;
            }
        }
    }
}

/// The lowest cell boundary at or below a coordinate.
fn align_down(value: I24F8) -> i32 {
    (value.to_bits() >> CELL_SHIFT) << CELL_SHIFT
}

/// The seven points a triangle's coverage is measured at.
fn coverage_samples(tri: &NavTri) -> [GlobalPoint; 7] {
    let [a, b, c] = tri.triangle();
    [
        a,
        b,
        c,
        blend([a, b], 2),
        blend([b, c], 2),
        blend([c, a], 2),
        blend([a, b, c], 3),
    ]
}

/// The average of some world positions.
///
/// Summed as bit patterns rather than as points: three earth radii is past what
/// a [`GlobalPoint`] holds, and the average of three of them is not.
fn blend<const N: usize>(points: [GlobalPoint; N], divisor: i64) -> GlobalPoint {
    let mut total = [0i64; 3];
    for point in points {
        for (axis, component) in point.to_array().iter().enumerate() {
            if let Some(slot) = total.get_mut(axis) {
                *slot += i64::from(component.to_bits());
            }
        }
    }
    GlobalPoint::new(
        I24F8::saturating_from_bits(total[0] / divisor),
        I24F8::saturating_from_bits(total[1] / divisor),
        I24F8::saturating_from_bits(total[2] / divisor),
    )
}
