//! The two directions this crate answers in, and the unit vector between them.
//!
//! **Every angle in this crate's public API is in degrees.** Right ascension
//! included: an almanac prints it in hours and this crate does not, because a
//! second type of angle in a crate whose whole job is angles is a conversion
//! waiting to be forgotten. Fifteen degrees is an hour.

use crate::math::{asin, atan2, cos, sin, wrap360};

/// A direction on the celestial sphere, and no distance.
///
/// Which equator and equinox it is measured against is not carried here; it is
/// documented on whatever produced it. Everything this crate hands out is
/// referred to the **true equator and equinox of date** unless it says
/// otherwise, which is the frame a horizon is computed in.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Equatorial {
    /// Right ascension, in **degrees** east along the equator, `0.0 ..= 360.0`.
    pub right_ascension: f64,
    /// Declination, in degrees north of the equator, `-90.0 ..= 90.0`.
    pub declination: f64,
}

impl Equatorial {
    /// A direction from its two angles, folded into range.
    #[must_use]
    pub fn new(right_ascension: f64, declination: f64) -> Self {
        Self {
            right_ascension: wrap360(right_ascension),
            declination: declination.clamp(-90.0, 90.0),
        }
    }

    /// The unit vector this direction is, with `+x` at the equinox, `+z` at the
    /// north pole.
    #[must_use]
    pub fn to_unit(self) -> [f64; 3] {
        let radius = cos(self.declination);
        [
            radius * cos(self.right_ascension),
            radius * sin(self.right_ascension),
            sin(self.declination),
        ]
    }

    /// The direction a vector points, which need not be normalised.
    #[must_use]
    pub fn from_unit(vector: [f64; 3]) -> Self {
        let [x, y, z] = vector;
        let length = libm::sqrt(x * x + y * y + z * z);
        if length == 0.0 {
            return Self {
                right_ascension: 0.0,
                declination: 0.0,
            };
        }
        Self {
            right_ascension: wrap360(atan2(y, x)),
            declination: asin(z / length),
        }
    }
}

/// Where a thing is in the sky as seen from one spot on the ground.
///
/// Azimuth is measured **clockwise from north**, so east is 90 and south is
/// 180. That is the navigator's convention rather than the astronomer's
/// south-based one, because everything else in a game -- a wind bearing, a
/// compass, a street heading -- is already north-based, and one convention in a
/// project beats the right one in a crate.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Horizontal {
    /// Altitude above the horizon, in degrees. Negative is below it.
    pub altitude: f64,
    /// Azimuth, in degrees clockwise from north, `0.0 ..= 360.0`.
    pub azimuth: f64,
}

impl Horizontal {
    /// A horizontal direction from its two angles, folded into range.
    #[must_use]
    pub fn new(altitude: f64, azimuth: f64) -> Self {
        Self {
            altitude: altitude.clamp(-90.0, 90.0),
            azimuth: wrap360(azimuth),
        }
    }

    /// Whether the thing is above the horizon at all.
    ///
    /// Geometry, not visibility. The sun is up by this test at noon and so is
    /// a fourth-magnitude star.
    #[must_use]
    pub fn is_up(self) -> bool {
        self.altitude > 0.0
    }

    /// The unit vector this direction is, in the local **east-north-up** frame:
    /// `+x` east, `+y` north, `+z` up.
    #[must_use]
    pub fn to_unit(self) -> [f64; 3] {
        let radius = cos(self.altitude);
        [
            radius * sin(self.azimuth),
            radius * cos(self.azimuth),
            sin(self.altitude),
        ]
    }
}
