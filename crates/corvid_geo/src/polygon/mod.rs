//! Polygons with holes, and the triangles they are cut into.

mod diagonal;
mod earcut;
mod nodes;
mod predicate;
mod ring;

use alloc::vec::Vec;

use corvid_fixed::I48F16;

use crate::polygon::predicate::{cross, doubled_area, half_area};
use crate::{GroundPoint, Winding};

pub use ring::Ring;

/// An outer ring and the holes punched in it.
///
/// A block with a courtyard, a parcel with a light well, a park with a pond:
/// the shape a map is actually made of. Construction reorients whatever it is
/// handed, so the outer ring always runs counterclockwise and every hole
/// clockwise however the archive stored them.
///
/// ```
/// use corvid_geo::{ground, Polygon, Ring};
/// use corvid_fixed::I48F16;
///
/// let block = Polygon::new(
///     Ring::new(vec![ground(0, 0), ground(30, 0), ground(30, 30), ground(0, 30)]),
///     vec![Ring::new(vec![
///         ground(10, 10), ground(20, 10), ground(20, 20), ground(10, 20),
///     ])],
/// );
///
/// // Nine hundred square metres of block, one hundred of courtyard.
/// assert_eq!(block.signed_area(), I48F16::from_f64(800.0));
/// assert!(block.contains(ground(5, 5)));
/// assert!(!block.contains(ground(15, 15)));
///
/// let cut = block.triangulate().expect("a courtyard is a hole like any other");
/// assert_eq!(cut.area(), block.signed_area());
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Polygon {
    outer: Ring,
    holes: Vec<Ring>,
}

impl Polygon {
    /// A polygon from an outer ring and its holes, each reoriented to the
    /// convention.
    #[must_use]
    pub fn new(outer: Ring, holes: Vec<Ring>) -> Self {
        Self {
            outer: outer.oriented(Winding::Counterclockwise),
            holes: holes
                .into_iter()
                .map(|hole| hole.oriented(Winding::Clockwise))
                .collect(),
        }
    }

    /// The outer ring, counterclockwise.
    #[must_use]
    #[inline]
    pub const fn outer(&self) -> &Ring {
        &self.outer
    }

    /// The holes, each clockwise.
    #[must_use]
    #[inline]
    pub fn holes(&self) -> &[Ring] {
        &self.holes
    }

    /// The area enclosed, in square metres, with the holes taken out.
    ///
    /// The holes run clockwise, so their signed areas are negative and the sum
    /// is the subtraction. Every ring is accumulated at the points' own scale
    /// and the halving rounds once at the end, so this is exactly what
    /// [`Triangulation::area`] answers for the triangles it cuts.
    #[must_use]
    pub fn signed_area(&self) -> I48F16 {
        let doubled = core::iter::once(&self.outer)
            .chain(&self.holes)
            .map(|ring| doubled_area(ring.points()))
            .sum();
        half_area(doubled)
    }

    /// Whether a point is inside the polygon or on its boundary.
    ///
    /// A point in a courtyard is outside; a point on the courtyard's wall is
    /// on the boundary and therefore inside, which is the rule that keeps a
    /// shared edge from belonging to neither shape.
    #[must_use]
    pub fn contains(&self, point: GroundPoint) -> bool {
        self.outer.contains(point)
            && self
                .holes
                .iter()
                .all(|hole| !hole.contains(point) || hole.on_boundary(point))
    }

    /// Cuts the polygon into triangles.
    ///
    /// # Errors
    ///
    /// [`Triangulate::Degenerate`] when the outer ring encloses no area,
    /// [`Triangulate::Unbridged`] when a hole is not inside the ring it was
    /// given to, and [`Triangulate::NotSimple`] when the boundary crosses
    /// itself. None of the three has a partition to answer with.
    pub fn triangulate(&self) -> Result<Triangulation, Triangulate> {
        earcut::triangulate(&self.outer, &self.holes)
    }
}

/// A polygon cut into triangles, deterministically.
///
/// The points are the polygon's own, outer ring first and each hole after it
/// in the order they were given, and a triangle names three of them. Bridging
/// a hole duplicates a *vertex* of the boundary rather than a point, so an
/// index here always addresses a point the polygon was built from -- which is
/// what lets a caller carry per-point data through the triangulation.
///
/// Every triangle is counterclockwise and none is degenerate, and their areas
/// sum to the polygon's exactly.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Triangulation {
    points: Vec<GroundPoint>,
    triangles: Vec<[u32; 3]>,
}

impl Triangulation {
    /// The points a triangle's indices name.
    #[must_use]
    #[inline]
    pub fn points(&self) -> &[GroundPoint] {
        &self.points
    }

    /// The triangles, each three indices into [`points`](Self::points),
    /// counterclockwise.
    #[must_use]
    #[inline]
    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    /// The total area of the triangles, in square metres.
    ///
    /// Equal to [`Polygon::signed_area`] for anything this crate triangulates,
    /// which is the cheapest honest check that the partition is a partition.
    #[must_use]
    pub fn area(&self) -> I48F16 {
        half_area(self.triangles.iter().map(|&t| self.doubled(t)).sum())
    }

    /// Twice the signed area of one triangle, in Q16 square metres.
    fn doubled(&self, triangle: [u32; 3]) -> i128 {
        match (
            self.points.get(triangle[0] as usize),
            self.points.get(triangle[1] as usize),
            self.points.get(triangle[2] as usize),
        ) {
            (Some(&a), Some(&b), Some(&c)) => cross(a, b, c),
            _ => 0,
        }
    }

    /// Twice the signed area of one triangle, for a caller checking the
    /// partition itself.
    #[must_use]
    pub fn triangle_area(&self, triangle: [u32; 3]) -> I48F16 {
        half_area(self.doubled(triangle))
    }
}

/// Why a polygon could not be cut into triangles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
pub enum Triangulate {
    /// The outer ring encloses no area, so there is nothing to cut.
    #[error("the outer ring encloses no area")]
    Degenerate,
    /// A hole could not be joined to its outer ring, which means it is not
    /// inside it.
    #[error("a hole is not inside the ring it was given to")]
    Unbridged,
    /// The boundary crosses itself, and a self-crossing boundary has no
    /// interior to partition.
    #[error("the boundary crosses itself")]
    NotSimple,
}
