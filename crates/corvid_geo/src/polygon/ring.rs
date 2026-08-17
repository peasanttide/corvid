//! A closed sequence of ground points, and what can be asked of one.

use alloc::vec::Vec;

use corvid_fixed::I48F16;

use crate::polygon::predicate::{doubled_area, half_area, on_segment, winding_number};
use crate::{GroundPoint, Winding};

/// A closed ring of ground points.
///
/// The closing edge is implied: a ring of three points has three edges, and
/// repeating the first point at the end would make the last of them
/// degenerate. Nothing is validated on construction, because an archive's ring
/// is what it is and the useful moment to complain is when somebody asks a
/// question it cannot answer -- [`winding`](Self::winding) says `None` for a
/// ring with no area, and
/// [`Polygon::triangulate`](crate::Polygon::triangulate) says so with an
/// error.
///
/// ```
/// use corvid_geo::{ground, Ring, Winding};
/// use corvid_fixed::I48F16;
///
/// let square = Ring::new(vec![
///     ground(0, 0), ground(10, 0), ground(10, 10), ground(0, 10),
/// ]);
///
/// assert_eq!(square.signed_area(), I48F16::from_f64(100.0));
/// assert_eq!(square.winding(), Some(Winding::Counterclockwise));
/// assert!(square.contains(ground(5, 5)));
/// assert!(!square.contains(ground(15, 5)));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Ring(Vec<GroundPoint>);

impl Ring {
    /// A ring from its points, in order, without the first repeated at the
    /// end.
    #[must_use]
    #[inline]
    pub const fn new(points: Vec<GroundPoint>) -> Self {
        Self(points)
    }

    /// The points, in order.
    #[must_use]
    #[inline]
    pub fn points(&self) -> &[GroundPoint] {
        &self.0
    }

    /// The points, taken back out.
    #[must_use]
    #[inline]
    pub fn into_points(self) -> Vec<GroundPoint> {
        self.0
    }

    /// How many points, which is also how many edges.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no points at all.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The signed area, in square metres: positive counterclockwise, negative
    /// clockwise.
    ///
    /// Exact for any ring whose area stays inside [`I48F16`], because the
    /// shoelace sum is accumulated in an `i128` at the points' own Q16 scale
    /// and rounded exactly once, at the halving.
    #[must_use]
    pub fn signed_area(&self) -> I48F16 {
        half_area(doubled_area(&self.0))
    }

    /// Which way round the ring runs, or `None` when it encloses nothing.
    #[must_use]
    pub fn winding(&self) -> Option<Winding> {
        match doubled_area(&self.0) {
            0 => None,
            area if area > 0 => Some(Winding::Counterclockwise),
            _ => Some(Winding::Clockwise),
        }
    }

    /// The same ring, running the other way.
    #[must_use]
    pub fn reversed(mut self) -> Self {
        self.0.reverse();
        self
    }

    /// The same ring, running the way asked for.
    ///
    /// A ring with no area is returned untouched, since it has no winding to
    /// correct.
    #[must_use]
    pub fn oriented(self, winding: Winding) -> Self {
        match self.winding() {
            Some(current) if current != winding => self.reversed(),
            _ => self,
        }
    }

    /// Whether a point is inside the ring or on its boundary.
    ///
    /// By winding number rather than by crossing count, so a ring that laps
    /// itself encloses its middle both times rather than alternately. The
    /// boundary counts as inside, which is the rule that makes
    /// [`Polygon::contains`](crate::Polygon::contains) agree with itself on a
    /// shared edge between a courtyard and the block around it.
    #[must_use]
    pub fn contains(&self, point: GroundPoint) -> bool {
        self.on_boundary(point) || winding_number(&self.0, point) != 0
    }

    /// Whether a point lies exactly on one of the ring's edges.
    #[must_use]
    pub fn on_boundary(&self, point: GroundPoint) -> bool {
        let count = self.0.len();
        (0..count).any(
            |index| match (self.0.get(index), self.0.get((index + 1) % count)) {
                (Some(&a), Some(&b)) => on_segment(a, b, point),
                _ => false,
            },
        )
    }
}
