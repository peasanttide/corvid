//! The coarse grid that guesses where a query starts.

use alloc::vec::Vec;

use corvid_fixed::I24F8;
use corvid_vector::GlobalPoint;

use crate::cords::NavTriRef;
use crate::error::NavError;
use crate::plane::NavPlane;
use crate::tri::NavTri;

/// The most cell-and-triangle pairs a grid may hold.
///
/// Twelve bytes each, so the cap is a hundred megabytes and a mesh that reaches
/// it is one whose triangles are far finer than its pitch. The answer to
/// [`NavError::GridTooLarge`] is a coarser [`Tune::grid_pitch`](crate::Tune),
/// which costs a longer walk per query and nothing else.
const MAX_ENTRIES: usize = 1 << 23;

/// How far out from an empty cell a lookup will look.
const SEARCH_RINGS: i32 = 2;

/// One square of the level's tangent plane.
///
/// East and north in whole pitches from the plane's origin, and signed because
/// a caller may ask about somewhere the level is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NavCell {
    /// How many pitches east of the plane's origin.
    pub east: i32,
    /// How many pitches north of it.
    pub north: i32,
}

/// Where to start looking for the triangle under a point.
///
/// A **sparse** index of the level's **tangent plane**, at a pitch a game
/// chooses and 32 m by default. Both of those are the design: a dense
/// three-dimensional array over an ECEF bounding box charges a city for the sky
/// above it and for the diagonal a level plane cuts through all three ECEF
/// axes, and at Titonville that put the ceiling on a district at 2,464 m on a
/// side. A grid that holds only the cells the ground reaches, in the plane the
/// ground is flat in, has no such ceiling.
///
/// Each cell holds every triangle whose corners' bounding box covers it, in
/// triangle order. That is deliberately more than one: a query then has real
/// candidates to test rather than one guess to correct, and
/// [`NavMesh::locate`](crate::NavMesh::locate) walks from whichever of them is
/// nearest being right. The grid is still allowed to be wrong -- `tests/fold.rs`
/// starts a walk from a deliberately bad guess to prove the answer does not
/// depend on it.
///
/// [`rebuild_cell`](Self::rebuild_cell) is the other half of the design. An
/// editor that moves one building re-cuts the cells that building is in, not
/// the city.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct NavGrid {
    plane: NavPlane,
    base: [i32; 2],
    pitch: I24F8,
    cells: Vec<(i32, i32, u32)>,
}

impl NavGrid {
    /// The cell size a level gets if it does not ask for another.
    ///
    /// Thirty-two metres: four or five buildings of a Paris street front, which
    /// is coarse enough that a district's grid is thousands of cells rather
    /// than millions and fine enough that a cell holds a handful of triangles
    /// rather than a quarter.
    pub const DEFAULT_PITCH: I24F8 = I24F8::from_bits(32 << 8);

    /// The least east and the least north the surface reaches, in the units
    /// [`NavPlane::offsets`] answers.
    ///
    /// What cell zero is measured from, so that a level's own south-west corner
    /// is the grid's origin rather than whatever corner its ECEF bounding box
    /// happened to have.
    #[must_use]
    #[inline]
    pub const fn base(&self) -> [i32; 2] {
        self.base
    }

    /// The plane the cells are laid out in.
    #[must_use]
    #[inline]
    pub const fn plane(&self) -> NavPlane {
        self.plane
    }

    /// How wide a cell is, in metres.
    #[must_use]
    #[inline]
    pub const fn pitch(&self) -> I24F8 {
        self.pitch
    }

    /// How many cell-and-triangle pairs the grid holds.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether the grid holds nothing at all.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Which cell a world position falls in.
    ///
    /// Unclamped, so a point outside the level keeps its direction rather than
    /// folding onto an edge, and floored rather than truncated, so the cells
    /// are the same width on both sides of the origin.
    #[must_use]
    #[inline]
    pub fn cell_of(&self, point: GlobalPoint) -> NavCell {
        let [east, north] = self.plane.offsets(point);
        let pitch = self.pitch.to_bits();
        NavCell {
            east: east.saturating_sub(self.base[0]).div_euclid(pitch),
            north: north.saturating_sub(self.base[1]).div_euclid(pitch),
        }
    }

    /// Every triangle in a cell, in triangle order.
    pub fn tris_in(&self, cell: NavCell) -> impl Iterator<Item = NavTriRef> {
        let range = self.range_of(cell);
        self.cells[range].iter().map(|&(_, _, tri)| NavTriRef(tri))
    }

    /// Every triangle worth testing for a point, nearest cell first.
    ///
    /// The point's own cell, then the ring around it, and so on out to two
    /// rings. A caller that wants one answer takes [`lookup`](Self::lookup);
    /// this is for one that can tell a right answer from a near one, which
    /// [`NavMesh::locate`](crate::NavMesh::locate) can.
    pub fn candidates(&self, point: GlobalPoint) -> impl Iterator<Item = NavTriRef> {
        let home = self.cell_of(point);
        (0..=SEARCH_RINGS)
            .flat_map(move |ring| ring_cells(home, ring).flat_map(move |cell| self.tris_in(cell)))
    }

    /// The triangle to start a query for `point` from, or [`None`] if no cell
    /// within two of it holds one.
    #[must_use]
    pub fn lookup(&self, point: GlobalPoint) -> Option<NavTriRef> {
        self.candidates(point).next()
    }

    /// Re-cuts one cell, from the triangles it already held and the ones a
    /// caller says are new.
    ///
    /// **This is what an editor calls.** Moving a building changes a few
    /// triangles and leaves a quarter of a million alone, so the index for the
    /// cells that building stands in is rebuilt from the union of what was
    /// there and what has arrived, and no other cell is touched. A triangle
    /// that has stopped covering the cell drops out because the test is applied
    /// again to everybody, not because the caller remembered to say so.
    ///
    /// `tris` is the mesh's triangles as they are *now*; `added` names any that
    /// were not in the cell before. Both are read, neither is kept.
    pub fn rebuild_cell(&mut self, tris: &[NavTri], cell: NavCell, added: &[NavTriRef]) {
        let range = self.range_of(cell);
        let mut held: Vec<u32> = self.cells[range.clone()]
            .iter()
            .map(|&(_, _, tri)| tri)
            .collect();
        held.extend(added.iter().map(|reference| reference.0));
        held.sort_unstable();
        held.dedup();

        let mut fresh = Vec::with_capacity(held.len());
        for reference in held {
            let Some(tri) = tris.get(reference as usize) else {
                continue;
            };
            let (low, high) = self.span_of(tri);
            if cell.east >= low.east
                && cell.east <= high.east
                && cell.north >= low.north
                && cell.north <= high.north
            {
                fresh.push((cell.east, cell.north, reference));
            }
        }
        self.cells.splice(range, fresh);
    }

    /// Builds the grid over a mesh's triangles.
    ///
    /// # Errors
    ///
    /// [`NavError::GridTooLarge`] when the triangles cover more cells between
    /// them than the grid is allowed to hold, which is a mesh whose faces are
    /// far finer than its pitch.
    pub(crate) fn build(tris: &[NavTri], pitch: I24F8) -> Result<Self, NavError> {
        let Some(first) = tris.first() else {
            return Ok(Self::default());
        };
        let mut low = first.triangle()[0];
        let mut high = low;
        for tri in tris {
            for vertex in tri.triangle() {
                low = low.min(vertex);
                high = high.max(vertex);
            }
        }

        // The plane's origin is the box's low corner in ECEF, which is not a
        // point of the level and does not project to its south-west corner. The
        // base is what does: the least east and the least north anything on the
        // surface reaches, so the level starts at cell zero and no cell of it
        // is negative.
        let plane = NavPlane::over(low, high);
        let mut base = [i32::MAX; 2];
        for tri in tris {
            for vertex in tri.triangle() {
                let [east, north] = plane.offsets(vertex);
                base[0] = base[0].min(east);
                base[1] = base[1].min(north);
            }
        }
        let mut grid = Self {
            plane,
            base,
            pitch: if pitch.to_bits() > 0 {
                pitch
            } else {
                Self::DEFAULT_PITCH
            },
            cells: Vec::with_capacity(tris.len()),
        };

        for (index, tri) in tris.iter().enumerate() {
            let reference = u32::try_from(index).unwrap_or(u32::MAX);
            let (from, to) = grid.span_of(tri);
            let across = i64::from(to.east - from.east + 1) * i64::from(to.north - from.north + 1);
            if across + grid.cells.len() as i64 > MAX_ENTRIES as i64 {
                return Err(NavError::GridTooLarge {
                    cells: (across + grid.cells.len() as i64).unsigned_abs(),
                    limit: MAX_ENTRIES as u64,
                });
            }
            for north in from.north..=to.north {
                for east in from.east..=to.east {
                    grid.cells.push((east, north, reference));
                }
            }
        }
        // Sorted on the whole tuple, so a cell's triangles are in triangle
        // order and the first of them is a fact about the mesh rather than
        // about the order the faces happened to be scanned in.
        grid.cells.sort_unstable();
        Ok(grid)
    }

    /// The cells a triangle's corners reach between them.
    fn span_of(&self, tri: &NavTri) -> (NavCell, NavCell) {
        let [a, b, c] = tri.triangle();
        let mut low = self.cell_of(a);
        let mut high = low;
        for vertex in [b, c] {
            let cell = self.cell_of(vertex);
            low.east = low.east.min(cell.east);
            low.north = low.north.min(cell.north);
            high.east = high.east.max(cell.east);
            high.north = high.north.max(cell.north);
        }
        (low, high)
    }

    /// Where a cell's entries sit in the sorted list.
    fn range_of(&self, cell: NavCell) -> core::ops::Range<usize> {
        let start = self
            .cells
            .partition_point(|&(east, north, _)| (east, north) < (cell.east, cell.north));
        let end = self
            .cells
            .partition_point(|&(east, north, _)| (east, north) <= (cell.east, cell.north));
        start..end
    }
}

/// The cells exactly `ring` steps from `home`, in a fixed order.
fn ring_cells(home: NavCell, ring: i32) -> impl Iterator<Item = NavCell> {
    (-ring..=ring).flat_map(move |north| {
        (-ring..=ring).filter_map(move |east| {
            (east.abs().max(north.abs()) == ring).then_some(NavCell {
                east: home.east + east,
                north: home.north + north,
            })
        })
    })
}
