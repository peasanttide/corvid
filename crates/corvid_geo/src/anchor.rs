//! The local east-north-up frame a level's own metres are measured in.

use corvid_vector::{Direction, GlobalFinePoint, GlobalPoint};

use crate::arith::UNIT;
use crate::{Ellipsoid, Geodetic};

/// A stated geodetic origin and the east-north-up basis standing on it.
///
/// This is how a level addresses its own ground. The anchor is authored once,
/// as a latitude and a longitude somebody can look up; everything after it is
/// a [`GlobalPoint`] offset in metres east, north and up from that spot, and
/// no floating point appears anywhere in the trip. A level built this way has
/// its own origin at `(0, 0, 0)`, so its coordinates read as the tape measure
/// read, while [`to_ecef`](Self::to_ecef) still places every one of them on
/// the real earth.
///
/// The basis is exact in the sense that matters: east, north and up are
/// [`Direction`]s, unit vectors at `4.7e-10` a component, and the tangent
/// plane they span touches the ellipsoid at the origin. A point ten kilometres
/// away is therefore about 8 metres above the curved surface, which is what a
/// tangent plane means and is why an anchor belongs to a level rather than to
/// a world.
///
/// ```
/// use corvid_fixed::{Angle32, I24F8, Pitch32};
/// use corvid_geo::{Anchor, Ellipsoid, Geodetic};
/// use corvid_vector::{GlobalPoint, globalpoint};
///
/// let titon = Geodetic::new(
///     Pitch32::from_degrees(48.8524),
///     Angle32::from_degrees(2.3855),
///     I24F8::ZERO,
/// );
/// let anchor = Anchor::new(titon, Ellipsoid::WGS84);
///
/// // A hundred metres east of the anchor, and back again.
/// let corner = globalpoint(100, 0, 0);
/// let world = anchor.to_ecef(corner).expect("a hundred metres is not far");
/// let local = anchor.to_local(world).expect("and it comes back");
///
/// assert!(local.distance(corner) <= I24F8::from_f64(0.02));
/// assert_eq!(anchor.origin(), titon);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Anchor {
    origin: Geodetic,
    ecef: GlobalFinePoint,
    east: Direction,
    north: Direction,
    up: Direction,
}

impl Anchor {
    /// The frame standing on a geodetic origin.
    ///
    /// The origin is kept at [`GlobalFinePoint`]'s 15.26 um rather than at a
    /// world position's 3.9 mm, because every offset in the level is measured
    /// from it and an error there is an error in all of them at once.
    #[must_use]
    pub fn new(origin: Geodetic, ellipsoid: Ellipsoid) -> Self {
        let (sin_lat, cos_lat) = origin.latitude().sin_cos();
        let (sin_lon, cos_lon) = origin.longitude().sin_cos();
        let (sin_lat, cos_lat) = (sin_lat.to_bits() as i64, cos_lat.to_bits() as i64);
        let (sin_lon, cos_lon) = (sin_lon.to_bits() as i64, cos_lon.to_bits() as i64);
        let unit = UNIT as i64;

        Self {
            origin,
            ecef: origin.to_ecef_fine(ellipsoid),
            // Each row is a ratio rather than a vector, so the common factor
            // of `UNIT` on the two-term rows is carried on the third to keep
            // all three at one scale. A product of two `Signed32` bit patterns
            // reaches `4.6e18`, inside an `i64` with room.
            east: unit_direction([-sin_lon * unit, cos_lon * unit, 0], Direction::X),
            north: unit_direction(
                [-sin_lat * cos_lon, -sin_lat * sin_lon, cos_lat * unit],
                Direction::Y,
            ),
            up: unit_direction(
                [cos_lat * cos_lon, cos_lat * sin_lon, sin_lat * unit],
                Direction::Z,
            ),
        }
    }

    /// The geodetic position the frame stands on.
    #[must_use]
    #[inline]
    pub const fn origin(&self) -> Geodetic {
        self.origin
    }

    /// The origin as an ECEF position.
    #[must_use]
    #[inline]
    pub const fn ecef(&self) -> GlobalFinePoint {
        self.ecef
    }

    /// The direction of increasing local x: due east, along the parallel.
    #[must_use]
    #[inline]
    pub const fn east(&self) -> Direction {
        self.east
    }

    /// The direction of increasing local y: due north, along the meridian.
    #[must_use]
    #[inline]
    pub const fn north(&self) -> Direction {
        self.north
    }

    /// The direction of increasing local z: the ellipsoid normal, which is up.
    #[must_use]
    #[inline]
    pub const fn up(&self) -> Direction {
        self.up
    }

    /// A local offset placed on the earth, or `None` when the result leaves
    /// [`GlobalPoint`]'s box.
    #[must_use]
    pub fn to_ecef(&self, local: GlobalPoint) -> Option<GlobalPoint> {
        let offset = self
            .east
            .along(local.x())
            .checked_add(self.north.along(local.y()))?
            .checked_add(self.up.along(local.z()))?;
        self.ecef.checked_add(offset.to_global_fine())?.to_global()
    }

    /// An ECEF position read in the local frame, or `None` when it is more
    /// than [`GlobalPoint`]'s range away from the anchor.
    ///
    /// The offset is taken at [`GlobalFinePoint`] width before it is narrowed,
    /// so the subtraction that cancels the earth's radius is exact and only
    /// the answer is rounded.
    #[must_use]
    pub fn to_local(&self, ecef: GlobalPoint) -> Option<GlobalPoint> {
        let offset = ecef.to_global_fine().checked_sub(self.ecef)?.to_global()?;
        Some(GlobalPoint::new(
            offset.project(self.east),
            offset.project(self.north),
            offset.project(self.up),
        ))
    }
}

/// Normalizes a ratio, falling back to an axis for the zero vector.
///
/// The fallback cannot be reached. Every row above is built from a sine and a
/// cosine of the same angle, and `sin^2 + cos^2 = 1` puts at least one of the
/// two above `0.7` -- but `Direction::from_ratio` answers an `Option` because
/// some caller somewhere hands it a zero, and this crate does not panic.
fn unit_direction(ratio: [i64; 3], fallback: Direction) -> Direction {
    match Direction::from_ratio(ratio) {
        Some(direction) => direction,
        None => fallback,
    }
}
