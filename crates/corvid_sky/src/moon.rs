//! Where the moon was, and how much of it was lit.
//!
//! Meeus, *Astronomical Algorithms*, 2nd ed., chapter 47, which is
//! Chapront-Touze and Chapront's ELP-2000/82 truncated to sixty terms in
//! longitude and distance and sixty in latitude. Meeus states the residual as
//! ten arcseconds in longitude, four in latitude and about a hundred metres in
//! distance -- a twentieth of the moon's own apparent radius. The phase and the
//! illuminated fraction are chapter 48.
//!
//! The moon is the body a game most often gets wrong, for two reasons that
//! have nothing to do with the series. Its parallax is a whole degree, so a
//! geocentric moon is in the wrong place by twice its own diameter; that
//! correction is [`Observer::topocentric`](crate::Observer::topocentric). And
//! its position is only half the question -- whether there was *light* is the
//! phase, which is [`illuminated_fraction`](Moon::illuminated_fraction).

use crate::coordinates::Equatorial;
use crate::frame::{from_ecliptic, nutation, true_obliquity};
use crate::math::{asin, atan2, cos, poly, sin, wrap180, wrap360};
use crate::moon_table::{LATITUDE, LONGITUDE};
use crate::sun::Sun;
use crate::time::Instant;

/// Kilometres in an astronomical unit, IAU 2012.
const KM_PER_AU: f64 = 149_597_870.7;

/// The Earth's equatorial radius in kilometres, as the parallax formula of
/// Meeus chapter 47 uses it.
const EARTH_RADIUS_KM: f64 = 6_378.14;

/// The moon's mean radius in kilometres, IAU 2015.
const MOON_RADIUS_KM: f64 = 1_737.4;

/// The mean synodic month in days: how fast the elongation runs, and therefore
/// the step a phase search takes.
const SYNODIC_DEGREES_PER_DAY: f64 = 360.0 / 29.530_588_861;

/// Where the moon was, geocentric and apparent, and what phase it was in.
///
/// Geocentric. The moon is the one body in the sky close enough for that to be
/// the wrong answer for an observer on the ground -- diurnal parallax moves it
/// by up to a degree, which is the difference between a crescent above the
/// rooftops and one that set twenty minutes ago. Use
/// [`Observer::topocentric`](crate::Observer::topocentric) before drawing it.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Moon {
    /// The apparent direction from the Earth's centre, referred to the true
    /// equator and equinox of date.
    pub equatorial: Equatorial,
    /// Apparent ecliptic longitude, in degrees.
    pub ecliptic_longitude: f64,
    /// Ecliptic latitude, in degrees. Within about 5.3 either way, which is
    /// why there is not an eclipse every month.
    pub ecliptic_latitude: f64,
    /// Distance from the Earth's centre to the moon's, in kilometres.
    pub distance: f64,
    /// Equatorial horizontal parallax, in degrees: the angle the Earth's
    /// equatorial radius subtends at the moon. Just under one degree.
    pub parallax: f64,
    /// Apparent radius of the disc seen from the Earth's centre, in degrees.
    pub angular_radius: f64,
    /// Apparent ecliptic longitude of the moon minus that of the sun, in
    /// degrees, `0.0 ..= 360.0`. Zero at new moon and 180 at full, and this is
    /// the quantity [`phase`](Self::phase) names.
    pub elongation: f64,
    /// The sun-moon-earth angle in degrees: 0 at full moon, 180 at new. This
    /// is the angle the illuminated fraction is a function of, and it is
    /// **not** the elongation -- the two differ by the moon's own parallax in
    /// the triangle, a few tenths of a degree.
    pub phase_angle: f64,
    /// The fraction of the disc that is lit, `0.0 ..= 1.0`.
    pub illuminated_fraction: f64,
    /// Position angle of the midpoint of the bright limb, in degrees east of
    /// north. Which way the horns point, which is what a print of the period
    /// gets visibly wrong when it is drawn from memory.
    pub bright_limb: f64,
}

impl Moon {
    /// The moon at a moment.
    #[must_use]
    pub fn at(instant: Instant) -> Self {
        let centuries = instant.centuries();
        let (longitude, latitude, distance) = series(centuries);
        let apparent_longitude = wrap360(longitude + nutation(centuries).longitude);
        let equatorial = from_ecliptic(apparent_longitude, latitude, true_obliquity(centuries));

        let sun = Sun::at(instant);
        let separation = separation(sun.equatorial, equatorial);
        let sun_distance = sun.distance * KM_PER_AU;
        // Meeus 48.3. The triangle is sun, earth, moon; the phase angle is at
        // the moon, and solving for it from the two sides and the included
        // angle is what makes this exact rather than `180 - elongation`.
        let phase_angle = wrap360(atan2(
            sun_distance * sin(separation),
            distance - sun_distance * cos(separation),
        ));

        Self {
            equatorial,
            ecliptic_longitude: apparent_longitude,
            ecliptic_latitude: latitude,
            distance,
            parallax: asin(EARTH_RADIUS_KM / distance),
            angular_radius: asin(MOON_RADIUS_KM / distance),
            elongation: wrap360(apparent_longitude - sun.ecliptic_longitude),
            phase_angle,
            illuminated_fraction: f64::midpoint(1.0, cos(phase_angle)),
            bright_limb: bright_limb(sun.equatorial, equatorial),
        }
    }

    /// Which eighth of the cycle the moon is in.
    #[must_use]
    pub fn phase(&self) -> Phase {
        Phase::from_elongation(self.elongation)
    }

    /// The moment nearest `instant` at which the moon's elongation from the
    /// sun is `target` degrees.
    ///
    /// Newton's method on the elongation, stepped by the *mean* synodic rate
    /// rather than the instantaneous one. The true rate swings between about
    /// 10.9 and 13.6 degrees a day, so a fixed slope still contracts, and the
    /// search converges to under a second in a handful of iterations without
    /// needing a derivative the series does not offer.
    ///
    /// The answer is the nearest such moment, which for a target of 0 is
    /// within about fifteen days either way. Ask for a specific lunation by
    /// starting inside it.
    #[must_use]
    pub fn elongation_near(instant: Instant, target: f64) -> Instant {
        let mut moment = instant;
        for _ in 0..24 {
            let error = wrap180(Self::at(moment).elongation - target);
            if libm::fabs(error) < 1e-9 {
                break;
            }
            moment = moment.shift_days(-error / SYNODIC_DEGREES_PER_DAY);
        }
        moment
    }

    /// The new moon nearest a moment: elongation zero.
    #[must_use]
    pub fn new_moon_near(instant: Instant) -> Instant {
        Self::elongation_near(instant, 0.0)
    }

    /// The full moon nearest a moment: elongation 180.
    #[must_use]
    pub fn full_moon_near(instant: Instant) -> Instant {
        Self::elongation_near(instant, 180.0)
    }
}

/// The eight names a phase goes by.
///
/// The cycle is cut into eight **equal** 45-degree sectors of elongation, so
/// [`Phase::New`] names the day and a half either side of new moon rather than
/// the instant of it. That is the division an almanac prints and a player
/// recognises. For the instant, ask [`Moon::new_moon_near`]; for how much light
/// there actually is, ask [`Moon::illuminated_fraction`], which is a number and
/// not a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Phase {
    /// Elongation within 22.5 degrees of 0. Nothing lit, and nothing up at
    /// night.
    New,
    /// Waxing, between 22.5 and 67.5 degrees. An evening crescent.
    WaxingCrescent,
    /// Elongation within 22.5 degrees of 90. Half lit, setting near midnight.
    FirstQuarter,
    /// Waxing, between 112.5 and 157.5 degrees.
    WaxingGibbous,
    /// Elongation within 22.5 degrees of 180. Up all night.
    Full,
    /// Waning, between 202.5 and 247.5 degrees.
    WaningGibbous,
    /// Elongation within 22.5 degrees of 270. Half lit, rising near midnight.
    LastQuarter,
    /// Waning, between 292.5 and 337.5 degrees. A morning crescent.
    WaningCrescent,
}

impl Phase {
    /// The sector an elongation in degrees falls in.
    #[must_use]
    pub fn from_elongation(elongation: f64) -> Self {
        // Offset by half a sector so that the four cardinal phases sit in the
        // middle of theirs rather than on the boundary between two.
        let sector = wrap360(elongation + 22.5);
        if sector < 45.0 {
            Self::New
        } else if sector < 90.0 {
            Self::WaxingCrescent
        } else if sector < 135.0 {
            Self::FirstQuarter
        } else if sector < 180.0 {
            Self::WaxingGibbous
        } else if sector < 225.0 {
            Self::Full
        } else if sector < 270.0 {
            Self::WaningGibbous
        } else if sector < 315.0 {
            Self::LastQuarter
        } else {
            Self::WaningCrescent
        }
    }
}

/// The ELP-2000/82 sum: apparent longitude, latitude and distance in
/// kilometres, all geocentric, longitude in the mean equinox of date.
fn series(centuries: f64) -> (f64, f64, f64) {
    let moon_longitude = poly(
        &[
            218.316_447_7,
            481_267.881_234_21,
            -0.001_578_6,
            1.0 / 538_841.0,
            -1.0 / 65_194_000.0,
        ],
        centuries,
    );
    let elongation = poly(
        &[
            297.850_192_1,
            445_267.111_403_4,
            -0.001_881_9,
            1.0 / 545_868.0,
            -1.0 / 113_065_000.0,
        ],
        centuries,
    );
    let sun_anomaly = poly(
        &[
            357.529_109_2,
            35_999.050_290_9,
            -0.000_153_6,
            1.0 / 24_490_000.0,
        ],
        centuries,
    );
    let moon_anomaly = poly(
        &[
            134.963_396_4,
            477_198.867_505_5,
            0.008_741_4,
            1.0 / 69_699.0,
            -1.0 / 14_712_000.0,
        ],
        centuries,
    );
    let argument = poly(
        &[
            93.272_095_0,
            483_202.017_523_3,
            -0.003_653_9,
            -1.0 / 3_526_000.0,
            1.0 / 863_310_000.0,
        ],
        centuries,
    );

    // The three additive arguments Meeus carries outside the tables: Venus,
    // Jupiter, and the flattening of the Earth.
    let venus = 119.75 + 131.849 * centuries;
    let jupiter = 53.09 + 479_264.290 * centuries;
    let flattening = 313.45 + 481_266.484 * centuries;
    // The sun's orbit is an ellipse and the series was fitted around a circle,
    // so every term carrying the sun's anomaly is scaled by this once, and
    // twice for a doubled anomaly.
    let eccentricity = 1.0 - centuries * (0.002_516 + centuries * 0.000_007_4);

    let mut sum_longitude = 0.0;
    let mut sum_distance = 0.0;
    for &(d, m, moon, f, coefficient_l, coefficient_r) in &LONGITUDE {
        let angle = f64::from(d) * elongation
            + f64::from(m) * sun_anomaly
            + f64::from(moon) * moon_anomaly
            + f64::from(f) * argument;
        let scale = libm::pow(eccentricity, f64::from(m.abs()));
        sum_longitude += f64::from(coefficient_l) * scale * sin(angle);
        sum_distance += f64::from(coefficient_r) * scale * cos(angle);
    }
    let mut sum_latitude = 0.0;
    for &(d, m, moon, f, coefficient_b) in &LATITUDE {
        let angle = f64::from(d) * elongation
            + f64::from(m) * sun_anomaly
            + f64::from(moon) * moon_anomaly
            + f64::from(f) * argument;
        let scale = libm::pow(eccentricity, f64::from(m.abs()));
        sum_latitude += f64::from(coefficient_b) * scale * sin(angle);
    }

    sum_longitude +=
        3_958.0 * sin(venus) + 1_962.0 * sin(moon_longitude - argument) + 318.0 * sin(jupiter);
    sum_latitude += -2_235.0 * sin(moon_longitude)
        + 382.0 * sin(flattening)
        + 175.0 * sin(venus - argument)
        + 175.0 * sin(venus + argument)
        + 127.0 * sin(moon_longitude - moon_anomaly)
        - 115.0 * sin(moon_longitude + moon_anomaly);

    (
        wrap360(moon_longitude + sum_longitude / 1e6),
        sum_latitude / 1e6,
        385_000.56 + sum_distance / 1_000.0,
    )
}

/// The angle between two directions, in degrees.
fn separation(first: Equatorial, second: Equatorial) -> f64 {
    let [ax, ay, az] = first.to_unit();
    let [bx, by, bz] = second.to_unit();
    crate::math::acos(ax * bx + ay * by + az * bz)
}

/// Position angle of the moon's bright limb, Meeus equation 48.5.
fn bright_limb(sun: Equatorial, moon: Equatorial) -> f64 {
    let difference = sun.right_ascension - moon.right_ascension;
    wrap360(atan2(
        cos(sun.declination) * sin(difference),
        sin(sun.declination) * cos(moon.declination)
            - cos(sun.declination) * sin(moon.declination) * cos(difference),
    ))
}
