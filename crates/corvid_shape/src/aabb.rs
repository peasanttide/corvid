//! An axis-aligned box: the bound everything else is culled by.

use corvid_fixed::I24F8;

use crate::{
    Cast, Hit, Ray,
    project::{UNIT, divide, narrow, offset_bits},
};
use corvid_vector::{Direction, GlobalPoint};

/// An axis-aligned box, given by two corners.
///
/// The bound a mesh, a cell, a particle system or a whole region gets summarised
/// to. Two points and six comparisons, which is why every broad phase in
/// existence starts here.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct Aabb {
    /// The low corner.
    pub min: GlobalPoint,
    /// The high corner.
    pub max: GlobalPoint,
}

impl Aabb {
    /// The box that holds nothing.
    ///
    /// **Inverted on purpose**: `min` is at the top of the range and `max` at
    /// the bottom, which makes this the identity for [`merge`](Self::merge) and
    /// [`expand`](Self::expand). Folding an empty sequence of points therefore
    /// gives an empty box rather than a box containing the origin, which is the
    /// difference between "nothing to draw" and "one degenerate thing at the
    /// world's centre" -- and the second is a bug that only shows up as a
    /// culling miss much later.
    pub const EMPTY: Self = Self {
        min: GlobalPoint::from_array([I24F8::MAX; 3]),
        max: GlobalPoint::from_array([I24F8::MIN; 3]),
    };

    /// A box from two corners, taken as given.
    ///
    /// No sorting: a caller that hands them over the wrong way round gets an
    /// empty box, which [`is_empty`](Self::is_empty) reports and every method
    /// here handles. Silently swapping them would turn a mistake into a box
    /// that is merely somewhere unexpected.
    #[must_use]
    #[inline]
    pub const fn new(min: GlobalPoint, max: GlobalPoint) -> Self {
        Self { min, max }
    }

    /// A box of a given half-extent about a centre.
    #[must_use]
    #[inline]
    pub fn around(centre: GlobalPoint, half: GlobalPoint) -> Self {
        Self::new(centre - half, centre + half)
    }

    /// The smallest box holding every point given.
    ///
    /// [`EMPTY`](Self::EMPTY) for no points at all.
    #[must_use]
    pub fn from_points(points: impl IntoIterator<Item = GlobalPoint>) -> Self {
        points.into_iter().fold(Self::EMPTY, Self::expand)
    }

    /// Whether this box holds nothing.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        let (min, max) = (self.min.to_array(), self.max.to_array());
        min[0] > max[0] || min[1] > max[1] || min[2] > max[2]
    }

    /// Half the width on each axis.
    #[must_use]
    #[inline]
    pub fn half_extent(&self) -> GlobalPoint {
        // Widened before halving, not after. A box may legitimately be wider
        // than one component's range -- `[-6000 km, 6000 km]` is 12 000 km on
        // a component that stops at 8 388 -- and `self.max - self.min` would
        // clamp that to the range before the halving ever saw it, answering
        // 4 194 km for a box whose half extent is 6 000.
        let [x, y, z] = offset_bits(self.max, self.min);
        GlobalPoint::from_array([
            I24F8::from_bits(narrow(x / 2)),
            I24F8::from_bits(narrow(y / 2)),
            I24F8::from_bits(narrow(z / 2)),
        ])
    }

    /// The middle.
    ///
    /// Derived from the low corner and the half extent rather than averaging
    /// the two corners, so that a box and the box `around` its own centre and
    /// half extent agree.
    /// Plain point arithmetic, because the sum cannot leave the range: the
    /// centre lies between the corners, and both of those are points already.
    /// Only the *extent* needs widening, which is
    /// [`half_extent`](Self::half_extent)'s business rather than this one's.
    #[must_use]
    #[inline]
    pub fn centre(&self) -> GlobalPoint {
        self.min + self.half_extent()
    }

    /// Whether a point is inside, boundary included.
    ///
    /// The boundary is **inside**. A half-open box makes a point on a face
    /// shared by two adjacent cells belong to neither of them, which is a hole
    /// in every spatial index built out of these -- and the hole is exactly one
    /// unit in the last place wide, so it is found by a player and not by a
    /// test.
    #[must_use]
    pub fn contains(&self, point: GlobalPoint) -> bool {
        let (min, max, at) = (self.min.to_array(), self.max.to_array(), point.to_array());
        (0..3).all(|axis| min[axis] <= at[axis] && at[axis] <= max[axis])
    }

    /// Whether two boxes touch or overlap.
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        // An empty box holds no points, so it meets nothing -- and without this
        // the interval test below says otherwise: a reversed `[1, 0]` is inside
        // `[-1, 2]` by both of its comparisons.
        if self.is_empty() || other.is_empty() {
            return false;
        }
        let (amin, amax) = (self.min.to_array(), self.max.to_array());
        let (bmin, bmax) = (other.min.to_array(), other.max.to_array());
        (0..3).all(|axis| amin[axis] <= bmax[axis] && bmin[axis] <= amax[axis])
    }

    /// The smallest box holding both.
    #[must_use]
    #[inline]
    pub const fn merge(self, other: Self) -> Self {
        Self::new(self.min.min(other.min), self.max.max(other.max))
    }

    /// The smallest box holding this one and a point.
    #[must_use]
    #[inline]
    pub const fn expand(self, point: GlobalPoint) -> Self {
        Self::new(self.min.min(point), self.max.max(point))
    }
}

/// The slab test: three intervals intersected, and the widest entry against the
/// narrowest exit.
///
/// Per axis, the ray enters the slab at one bound and leaves at the other, and
/// the box is hit exactly when the three entry-to-exit intervals share a point.
/// Two cases are worth writing down because both are where a naive version goes
/// wrong:
///
/// - **A zero direction component.** The ray is parallel to that pair of faces
///   and never crosses either, so the axis constrains nothing if the origin is
///   already between them and rules the whole cast out if it is not. Dividing
///   anyway is the division by zero this branch exists to avoid.
/// - **An origin inside the box.** The entry is behind the ray, so the answer is
///   the *exit* -- the far wall, from the inside -- rather than a negative
///   distance handed to a caller that will place a cursor with it.
///
/// The normal is the axis whose bound decided the answer, and its sign is
/// settled by [`Hit::new`], which turns every normal to face the ray it was
/// found by.
impl Cast for Aabb {
    fn cast(&self, ray: Ray) -> Option<Hit> {
        // A ray that goes nowhere arrives nowhere. Without this, every slope is
        // zero, no axis divides and no axis constrains, so both sentinels
        // survive the loop -- and the exit sentinel is positive, which reads as
        // a hit at the far edge of the world.
        if ray.is_degenerate() {
            return None;
        }
        let (min, max) = (self.min.to_array(), self.max.to_array());
        let origin = ray.origin.to_array();
        let direction = ray.direction.to_array();

        // Q8, wide, because a bound minus an origin is two i32s apart.
        let mut entry: i128 = i128::from(i32::MIN);
        let mut exit: i128 = i128::from(i32::MAX);
        let mut entry_axis = 0usize;
        let mut exit_axis = 0usize;

        for axis in 0..3 {
            let slope = i128::from(direction[axis].to_bits());
            let start = i128::from(origin[axis].to_bits());
            let (low, high) = (
                i128::from(min[axis].to_bits()),
                i128::from(max[axis].to_bits()),
            );

            if slope == 0 {
                if start < low || start > high {
                    return None;
                }
                continue;
            }

            // Multiplying by the unit is what turns a Q8 over a Q31 back into
            // a Q8 -- the unit rather than a shift of 31, for the reason
            // `project`'s own module documents.
            let first = divide((low - start) * UNIT, slope);
            let second = divide((high - start) * UNIT, slope);
            let (near, far) = if slope < 0 {
                (second, first)
            } else {
                (first, second)
            };

            if near > entry {
                entry = near;
                entry_axis = axis;
            }
            if far < exit {
                exit = far;
                exit_axis = axis;
            }
            if entry > exit {
                return None;
            }
        }

        let (distance, axis) = if entry >= 0 {
            (entry, entry_axis)
        } else if exit >= 0 {
            (exit, exit_axis)
        } else {
            return None;
        };

        Some(Hit::new(
            ray,
            I24F8::from_bits(narrow(distance)),
            axis_normal(axis),
        ))
    }
}

/// The unit direction along one of the three axes.
///
/// Used only by the cast above, and only to name which pair of faces was hit.
#[must_use]
const fn axis_normal(axis: usize) -> Direction {
    match axis {
        0 => Direction::X,
        1 => Direction::Y,
        _ => Direction::Z,
    }
}
