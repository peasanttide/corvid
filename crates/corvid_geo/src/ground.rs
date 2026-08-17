//! The two-dimensional point a map polygon is drawn from.

use corvid_fixed::I24F8;
use corvid_vector::GlobalPoint;

/// A point on a level's ground plane: metres east and metres north of an
/// [`Anchor`](crate::Anchor).
///
/// A footprint, a parcel and a block are flat things, and carrying a height
/// through the polygon code would be a third coordinate that every predicate
/// has to agree to ignore. So the ground is its own type, at
/// [`GlobalPoint`]'s own 3.9 mm and its own 8388 km, and
/// [`to_local`](Self::to_local) is where a height is put back.
///
/// ```
/// use corvid_geo::{ground, GroundPoint};
/// use corvid_fixed::I24F8;
///
/// let corner = ground(12, -4);
/// assert_eq!(corner.east(), I24F8::from_f64(12.0));
/// assert_eq!(corner.north(), I24F8::from_f64(-4.0));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroundPoint {
    east: I24F8,
    north: I24F8,
}

impl GroundPoint {
    /// The origin of the ground plane, which is the anchor itself.
    pub const ORIGIN: Self = Self {
        east: I24F8::ZERO,
        north: I24F8::ZERO,
    };

    /// A point, from its two metre counts.
    #[must_use]
    #[inline]
    pub const fn new(east: I24F8, north: I24F8) -> Self {
        Self { east, north }
    }

    /// Metres east of the anchor.
    #[must_use]
    #[inline]
    pub const fn east(self) -> I24F8 {
        self.east
    }

    /// Metres north of the anchor.
    #[must_use]
    #[inline]
    pub const fn north(self) -> I24F8 {
        self.north
    }

    /// A local position with its height dropped.
    #[must_use]
    #[inline]
    pub const fn from_local(local: GlobalPoint) -> Self {
        Self::new(local.x(), local.y())
    }

    /// This point at a height, as a position in the anchor's local frame.
    #[must_use]
    #[inline]
    pub const fn to_local(self, up: I24F8) -> GlobalPoint {
        GlobalPoint::new(self.east, self.north, up)
    }

    /// The bit patterns, widened, which is the form every predicate here works
    /// in.
    pub(crate) const fn bits(self) -> (i64, i64) {
        (self.east.to_bits() as i64, self.north.to_bits() as i64)
    }
}

/// A [`GroundPoint`] from two whole metre counts.
///
/// The short spelling the tests and the doc examples are written in, matching
/// [`corvid_vector::globalpoint`].
#[must_use]
#[inline]
pub fn ground(east: impl Into<I24F8>, north: impl Into<I24F8>) -> GroundPoint {
    GroundPoint::new(east.into(), north.into())
}

/// Which way round a ring runs.
///
/// The convention here is the one every GIS format and every triangulator
/// agrees on: an outer ring is [`Winding::Counterclockwise`] and a hole is
/// [`Winding::Clockwise`], so a ring's signed area is positive exactly when it
/// encloses material. [`Polygon::new`](crate::Polygon::new) reorients whatever
/// it is handed, because an archive's rings arrive either way round and a
/// triangulation that trusted them would fold a courtyard inside out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Winding {
    /// Positive signed area: an outer boundary.
    Counterclockwise,
    /// Negative signed area: a hole.
    Clockwise,
}

impl Winding {
    /// The other one.
    #[must_use]
    #[inline]
    pub const fn reversed(self) -> Self {
        match self {
            Self::Counterclockwise => Self::Clockwise,
            Self::Clockwise => Self::Counterclockwise,
        }
    }
}
