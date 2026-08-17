//! Where the sun was, from the low-precision solar series.
//!
//! Meeus, *Astronomical Algorithms*, 2nd ed., chapter 25, "Solar Coordinates",
//! in its low-accuracy form: mean longitude, mean anomaly, the equation of the
//! centre to three terms, and the correction from true to apparent longitude.
//! Meeus states its accuracy as 0.01 degree, and `tests/almanac.rs` checks it
//! against his own worked example 25.b, which is VSOP87 truncated to the
//! milliarcsecond.
//!
//! Nothing better is worth the bytes here. Half a pixel at a sensible field of
//! view is around 0.02 degrees, and the sun's own disc is half a degree.

use crate::coordinates::Equatorial;
use crate::frame::{from_ecliptic, nutation, true_obliquity};
use crate::math::{ARCSECONDS_PER_DEGREE, cos, sin, wrap180, wrap360};
use crate::time::Instant;

/// The sun's apparent semidiameter at one astronomical unit, in arcseconds.
const SEMIDIAMETER: f64 = 959.63;

/// Where the sun was, geocentric and apparent.
///
/// Apparent means aberration and the nutation in longitude are already in it,
/// so this is the direction a telescope points rather than the direction the
/// geometry alone gives. Turning it into an altitude and an azimuth is
/// [`Observer::horizontal`](crate::Observer::horizontal), which is where the
/// site and the refraction come in.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Sun {
    /// The apparent direction, referred to the true equator and equinox of
    /// date.
    pub equatorial: Equatorial,
    /// Apparent ecliptic longitude, in degrees. This is the number the moon's
    /// elongation is measured from, and the one an almanac's "sun in Taurus"
    /// is.
    pub ecliptic_longitude: f64,
    /// Distance from the Earth's centre, in astronomical units.
    pub distance: f64,
    /// Apparent radius of the disc, in degrees. About 0.266.
    pub angular_radius: f64,
}

impl Sun {
    /// The sun at a moment.
    #[must_use]
    pub fn at(instant: Instant) -> Self {
        let centuries = instant.centuries();

        // Geometric mean longitude and mean anomaly, referred to the mean
        // equinox of date.
        let mean_longitude = Self::mean_longitude(centuries);
        let anomaly = wrap360(357.529_11 + centuries * (35_999.050_29 - centuries * 0.000_153_7));
        let eccentricity =
            0.016_708_634 - centuries * (0.000_042_037 + centuries * 0.000_000_126_7);

        // The equation of the centre: the whole of the difference between a
        // circular orbit and Kepler's, to the accuracy this series claims.
        let centre = (1.914_602 - centuries * (0.004_817 + centuries * 0.000_014)) * sin(anomaly)
            + (0.019_993 - centuries * 0.000_101) * sin(2.0 * anomaly)
            + 0.000_289 * sin(3.0 * anomaly);
        let true_longitude = mean_longitude + centre;
        let true_anomaly = anomaly + centre;

        // The radius vector, from the orbit rather than from a series.
        let distance = 1.000_001_018 * (1.0 - eccentricity * eccentricity)
            / (1.0 + eccentricity * cos(true_anomaly));

        // True to apparent: a fixed 0.005_69 degree for aberration, and the
        // nutation in longitude carried by the moon's ascending node.
        let node = 125.04 - 1_934.136 * centuries;
        let apparent_longitude = true_longitude - 0.005_69 - 0.004_78 * sin(node);

        Self {
            equatorial: from_ecliptic(apparent_longitude, 0.0, true_obliquity(centuries)),
            ecliptic_longitude: wrap360(apparent_longitude),
            distance,
            angular_radius: SEMIDIAMETER / (ARCSECONDS_PER_DEGREE * distance),
        }
    }

    /// The sun's geometric mean longitude at a moment, in degrees.
    ///
    /// Separated out because the equation of time needs the *mean* longitude
    /// where everything else needs the apparent one, and taking the wrong one
    /// is a sixteen-minute error that looks like a plausible answer.
    #[must_use]
    fn mean_longitude(centuries: f64) -> f64 {
        wrap360(280.466_46 + centuries * (36_000.769_83 + centuries * 0.000_303_2))
    }

    /// Apparent solar time minus mean solar time, in **minutes**.
    ///
    /// This is what carries a clock reading to a sundial reading, and it is the
    /// reason a level whose sources say "four in the morning" cannot simply
    /// treat that as four hours after midnight on any uniform clock. It runs
    /// between about -14 and +16 minutes over a year. Meeus equation 28.1.
    #[must_use]
    pub fn equation_of_time(instant: Instant) -> f64 {
        let centuries = instant.centuries();
        let sun = Self::at(instant);
        let difference =
            Self::mean_longitude(centuries) - 0.005_718_3 - sun.equatorial.right_ascension
                + nutation(centuries).longitude * cos(true_obliquity(centuries));
        // Four minutes to the degree, because the Earth turns fifteen degrees
        // an hour.
        wrap180(difference) * 4.0
    }
}
