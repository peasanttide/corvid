//! One moment, one place, and everything above it.

use crate::atmosphere::Atmosphere;
use crate::coordinates::{Equatorial, Horizontal};
use crate::moon::Moon;
use crate::observer::Observer;
use crate::star::Star;
use crate::sun::Sun;
use crate::time::Instant;

/// The sky over one spot at one instant.
///
/// A bundle rather than a computation: the sun and the moon are evaluated once
/// in [`new`](Self::new), the moon's topocentric direction with them, and
/// everything else on this type reads those. Building one per rendered frame is
/// the intended shape; building one per pixel is not.
///
/// ```
/// use corvid_sky::{Civil, Instant, Observer, Sky, Twilight};
///
/// let site = Observer::new(48.856_6, 2.337_2, 35.0)?;
/// let midnight = Instant::from_civil(Civil::new(1789, 4, 29, 0, 0, 0.0))?;
/// let sky = Sky::new(midnight, site);
///
/// // Deep in the night, and the sun says so.
/// assert_eq!(sky.twilight(), Twilight::Night);
/// assert!(!sky.sun_position().is_up());
/// # Ok::<(), corvid_sky::SkyError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Sky {
    instant: Instant,
    observer: Observer,
    sun: Sun,
    moon: Moon,
    moon_topocentric: Equatorial,
}

impl Sky {
    /// The sky at a moment as seen from a site.
    #[must_use]
    pub fn new(instant: Instant, observer: Observer) -> Self {
        let moon = Moon::at(instant);
        Self {
            instant,
            observer,
            sun: Sun::at(instant),
            moon,
            moon_topocentric: observer.topocentric(instant, moon.equatorial, moon.parallax),
        }
    }

    /// The moment this sky was evaluated at.
    #[must_use]
    pub const fn instant(&self) -> Instant {
        self.instant
    }

    /// The site this sky was evaluated for.
    #[must_use]
    pub const fn observer(&self) -> Observer {
        self.observer
    }

    /// The sun, geocentric and apparent.
    #[must_use]
    pub const fn sun(&self) -> Sun {
        self.sun
    }

    /// The moon, geocentric and apparent, with its phase.
    #[must_use]
    pub const fn moon(&self) -> Moon {
        self.moon
    }

    /// Where the sun is in the sky: refracted, so this is where it looks
    /// rather than where it is.
    #[must_use]
    pub fn sun_position(&self) -> Horizontal {
        self.observer.apparent(self.instant, self.sun.equatorial)
    }

    /// Where the moon is in the sky: topocentric **and** refracted, which are
    /// the two corrections that decide whether it is up at all.
    #[must_use]
    pub fn moon_position(&self) -> Horizontal {
        self.observer.apparent(self.instant, self.moon_topocentric)
    }

    /// Where a star is in the sky, with proper motion, precession, nutation,
    /// parallax, aberration and refraction all applied.
    #[must_use]
    pub fn star_position(&self, star: &Star) -> Horizontal {
        self.observer
            .apparent(self.instant, star.apparent(self.instant))
    }

    /// How bright a star appears from here, after the air has taken its cut.
    ///
    /// The catalogue magnitude plus [`Atmosphere::extinction`] at the star's
    /// apparent altitude. Larger is fainter, as magnitudes are.
    #[must_use]
    pub fn star_magnitude(&self, star: &Star, atmosphere: &Atmosphere) -> f64 {
        star.magnitude + atmosphere.extinction(self.star_position(star).altitude)
    }

    /// How far the sky has got into the night.
    ///
    /// From the sun's **geometric** altitude, which is the convention every
    /// twilight definition uses: the boundaries are angles between the horizon
    /// and the sun's centre, with no refraction in them.
    #[must_use]
    pub fn twilight(&self) -> Twilight {
        Twilight::from_solar_altitude(
            self.observer
                .horizontal(self.instant, self.sun.equatorial)
                .altitude,
        )
    }
}

/// How dark it is, by the four thresholds everyone agrees on.
///
/// These are definitions and not measurements. What light actually reaches the
/// ground also depends on the moon, on cloud, and -- in a game where things are
/// burning -- on the fires, and none of those is in here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Twilight {
    /// The sun is up: above `-0.8333` degrees, which is where its upper limb
    /// clears a refracted horizon.
    Day,
    /// Civil twilight, down to 6 degrees below the horizon. Outdoor work is
    /// still possible and the brightest stars are out.
    Civil,
    /// Nautical twilight, down to 12 degrees. The horizon at sea is still
    /// visible; on land nothing is, without a lamp.
    Nautical,
    /// Astronomical twilight, down to 18 degrees. Dark to anyone not looking
    /// for the last glow.
    Astronomical,
    /// The sun more than 18 degrees down. Whatever light there is comes from
    /// the moon, the stars, or something on fire.
    Night,
}

impl Twilight {
    /// The band a solar altitude in degrees falls in.
    #[must_use]
    pub fn from_solar_altitude(altitude: f64) -> Self {
        if altitude > -0.833_3 {
            Self::Day
        } else if altitude > -6.0 {
            Self::Civil
        } else if altitude > -12.0 {
            Self::Nautical
        } else if altitude > -18.0 {
            Self::Astronomical
        } else {
            Self::Night
        }
    }
}
