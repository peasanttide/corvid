//! A spot on the ground, and the four things it does to a direction.
//!
//! Sidereal time turns a right ascension into an hour angle; the latitude
//! turns that into an altitude; parallax moves a nearby body because the
//! observer is not at the Earth's centre; and refraction lifts everything near
//! the horizon. Miss the last two and the moon is a degree out and sunset is
//! two minutes early.

use crate::coordinates::{Equatorial, Horizontal};
use crate::error::SkyError;
use crate::frame::apparent_sidereal;
use crate::math::{asin, atan2, cos, sin, tan, wrap360};
use crate::sun::Sun;
use crate::time::Instant;

/// The flattening ratio of the reference ellipsoid, `b / a`, as the parallax
/// formulae of Meeus chapter 11 use it.
const POLAR_RATIO: f64 = 0.996_647_19;

/// The Earth's equatorial radius in metres, matching [`POLAR_RATIO`].
const EQUATORIAL_RADIUS_M: f64 = 6_378_140.0;

/// Standard sea-level pressure, in millibars.
const STANDARD_PRESSURE: f64 = 1_010.0;

/// Standard air temperature, in degrees Celsius.
const STANDARD_TEMPERATURE: f64 = 10.0;

/// Where somebody is standing, and what the air above them is doing.
///
/// ```
/// use corvid_sky::Observer;
///
/// // Longitude is positive **east**, which is the sign convention every
/// // modern coordinate reference system uses and the opposite of the one
/// // eighteenth-century French astronomy used.
/// let site = Observer::new(48.856_6, 2.337_2, 35.0)?;
/// assert!((site.latitude() - 48.856_6).abs() < 1e-9);
/// # Ok::<(), corvid_sky::SkyError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Observer {
    latitude: f64,
    longitude: f64,
    elevation: f64,
    pressure: f64,
    temperature: f64,
    /// `rho * sin(phi')`, the observer's distance from the equatorial plane in
    /// Earth radii. Cached because it is four transcendentals and it does not
    /// depend on the time.
    polar_offset: f64,
    /// `rho * cos(phi')`, the observer's distance from the polar axis in Earth
    /// radii.
    axial_offset: f64,
}

impl Observer {
    /// A site from a geodetic latitude, an east longitude and an elevation in
    /// metres above the ellipsoid.
    ///
    /// The air is set to 1010 millibars at 10 degrees Celsius, which is what
    /// the standard refraction tables assume; [`with_air`](Self::with_air)
    /// changes it.
    ///
    /// # Errors
    ///
    /// [`SkyError::Site`] when the latitude is outside `-90 ..= 90` degrees,
    /// the longitude outside `-360 ..= 360`, or the elevation is not finite.
    pub fn new(latitude: f64, longitude: f64, elevation: f64) -> Result<Self, SkyError> {
        if !(-90.0..=90.0).contains(&latitude)
            || !(-360.0..=360.0).contains(&longitude)
            || !elevation.is_finite()
        {
            return Err(SkyError::Site);
        }
        // Meeus chapter 11: the geodetic latitude is the normal to the
        // ellipsoid and the parallax wants the direction to the centre, which
        // differ by up to eleven arcminutes at latitude 45.
        let reduced = atan2(POLAR_RATIO * sin(latitude), cos(latitude));
        let height = elevation / EQUATORIAL_RADIUS_M;
        Ok(Self {
            latitude,
            longitude: wrap360(longitude),
            elevation,
            pressure: STANDARD_PRESSURE,
            temperature: STANDARD_TEMPERATURE,
            polar_offset: POLAR_RATIO * sin(reduced) + height * sin(latitude),
            axial_offset: cos(reduced) + height * cos(latitude),
        })
    }

    /// The same site with a different air pressure and temperature, which
    /// changes only the refraction.
    ///
    /// Pressure is in millibars and temperature in degrees Celsius. A cold
    /// dense night lifts the horizon: at 1030 millibars and freezing, the sun
    /// is refracted about a tenth of a degree more than the standard model
    /// gives, which is twenty seconds of sunset.
    #[must_use]
    pub const fn with_air(mut self, pressure: f64, temperature: f64) -> Self {
        self.pressure = pressure;
        self.temperature = temperature;
        self
    }

    /// Geodetic latitude in degrees, north positive.
    #[must_use]
    pub const fn latitude(&self) -> f64 {
        self.latitude
    }

    /// Longitude in degrees, **east positive**, `0.0 ..= 360.0`.
    #[must_use]
    pub const fn longitude(&self) -> f64 {
        self.longitude
    }

    /// Elevation above the ellipsoid, in metres.
    #[must_use]
    pub const fn elevation(&self) -> f64 {
        self.elevation
    }

    /// Air pressure in millibars.
    #[must_use]
    pub const fn pressure(&self) -> f64 {
        self.pressure
    }

    /// Air temperature in degrees Celsius.
    #[must_use]
    pub const fn temperature(&self) -> f64 {
        self.temperature
    }

    /// Local apparent sidereal time, in degrees.
    #[must_use]
    pub fn sidereal(&self, instant: Instant) -> f64 {
        wrap360(apparent_sidereal(instant) + self.longitude)
    }

    /// The hour angle of a right ascension, in degrees, `0.0 ..= 360.0`.
    ///
    /// Zero on the meridian and increasing westward, so a body rises at an
    /// hour angle a little under 360 and sets at a little over 0.
    #[must_use]
    pub fn hour_angle(&self, instant: Instant, right_ascension: f64) -> f64 {
        wrap360(self.sidereal(instant) - right_ascension)
    }

    /// A celestial direction as an altitude and an azimuth, with no refraction
    /// applied.
    ///
    /// Geometric. This is where the body *is*; [`apparent`](Self::apparent) is
    /// where it looks like it is.
    #[must_use]
    pub fn horizontal(&self, instant: Instant, direction: Equatorial) -> Horizontal {
        let angle = self.hour_angle(instant, direction.right_ascension);
        let (sin_declination, cos_declination) =
            (sin(direction.declination), cos(direction.declination));
        let (sin_latitude, cos_latitude) = (sin(self.latitude), cos(self.latitude));
        Horizontal::new(
            asin(sin_latitude * sin_declination + cos_latitude * cos_declination * cos(angle)),
            atan2(
                -cos_declination * sin(angle),
                sin_declination * cos_latitude - cos_declination * sin_latitude * cos(angle),
            ),
        )
    }

    /// Atmospheric refraction at a **true** altitude, in degrees.
    ///
    /// Saemundsson's formula as Meeus prints it at equation 16.4, scaled for
    /// pressure and temperature. It answers 0.568 degrees at a true altitude of
    /// zero and 0.573 at a true altitude of -34 arcminutes -- which is the
    /// standard 34 arcminutes stated the way round that makes it usable: a body
    /// whose true altitude is -34 arcminutes is seen *on* the horizon.
    ///
    /// Below a true altitude of -2 degrees the fit stops meaning anything, so
    /// the argument is clamped there. Nothing is visible that far under the
    /// horizon anyway.
    #[must_use]
    pub fn refraction(&self, altitude: f64) -> f64 {
        let clamped = altitude.max(-2.0);
        let arcminutes = 1.02 / tan(clamped + 10.3 / (clamped + 5.11)) + 0.001_927_9;
        arcminutes / 60.0
            * (self.pressure / STANDARD_PRESSURE)
            * (283.0 / (273.0 + self.temperature))
    }

    /// A celestial direction as it is *seen*: horizontal, then lifted by
    /// refraction.
    #[must_use]
    pub fn apparent(&self, instant: Instant, direction: Equatorial) -> Horizontal {
        let geometric = self.horizontal(instant, direction);
        Horizontal::new(
            geometric.altitude + self.refraction(geometric.altitude),
            geometric.azimuth,
        )
    }

    /// A geocentric direction moved to where this site sees it, given the
    /// body's equatorial horizontal parallax in degrees.
    ///
    /// Meeus equations 40.2 and 40.3. This matters for exactly one body: the
    /// moon's parallax is nearly a degree, twice its own diameter, and it is
    /// the difference between a crescent above the rooftops and one that has
    /// already set. For the sun it is nine arcseconds and for a star it is
    /// nothing at all.
    #[must_use]
    pub fn topocentric(
        &self,
        instant: Instant,
        direction: Equatorial,
        parallax: f64,
    ) -> Equatorial {
        let angle = self.hour_angle(instant, direction.right_ascension);
        let sin_parallax = sin(parallax);
        let (sin_declination, cos_declination) =
            (sin(direction.declination), cos(direction.declination));
        let denominator = cos_declination - self.axial_offset * sin_parallax * cos(angle);
        let shift = atan2(-self.axial_offset * sin_parallax * sin(angle), denominator);
        Equatorial::new(
            direction.right_ascension + shift,
            atan2(
                (sin_declination - self.polar_offset * sin_parallax) * cos(shift),
                denominator,
            ),
        )
    }

    /// The local apparent solar clock, in hours `0.0 ..= 24.0`.
    ///
    /// The sundial, not the wall clock: mean time carried across by the
    /// equation of time and the site's own longitude. In 1789 there were no
    /// time zones and no railway to impose one, so this is what the church
    /// bells and every witness in a period source are on.
    #[must_use]
    pub fn apparent_solar(&self, instant: Instant) -> f64 {
        let fraction = instant.universal() + 0.5;
        let universal_hours = (fraction - libm::floor(fraction)) * 24.0;
        let hours = universal_hours + self.longitude / 15.0 + Sun::equation_of_time(instant) / 60.0;
        hours - 24.0 * libm::floor(hours / 24.0)
    }

    /// The moment nearest `instant` at which the local apparent solar clock
    /// reads `hours`.
    ///
    /// Nearest, so it is within twelve hours either way. The apparent clock
    /// runs at very nearly the rate of the universal one -- the equation of
    /// time moves by under half a minute a day -- so this converges in two
    /// steps.
    #[must_use]
    pub fn apparent_solar_near(&self, instant: Instant, hours: f64) -> Instant {
        let mut moment = instant;
        for _ in 0..8 {
            let mut error = self.apparent_solar(moment) - hours;
            if error > 12.0 {
                error -= 24.0;
            } else if error < -12.0 {
                error += 24.0;
            }
            if libm::fabs(error) < 1e-9 {
                break;
            }
            moment = moment.shift_seconds(-error * 3_600.0);
        }
        moment
    }
}
