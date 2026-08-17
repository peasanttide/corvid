//! A catalogue entry, and the four things between it and where the star was.
//!
//! A catalogue records a star at an epoch, and rolling it back to 1789 is a
//! computation rather than a fudge. Four corrections, in this order, and the
//! famous one is the smallest:
//!
//! **Space motion.** The star has moved. Proper motion is at most ten
//! arcseconds a year and for most stars is a hundredth of that, so over 211
//! years it ranges from invisible to two thirds of a degree.
//!
//! **Precession and nutation.** The *frame* has moved, by about 50.3
//! arcseconds a year, so nearly three degrees since J2000 -- six solar
//! diameters, and a hundred times proper motion. Skip it and every
//! constellation rises in the wrong place. See [`crate::frame`].
//!
//! **Annual parallax.** The Earth is not at the sun. Under an arcsecond for
//! every star there is, and included because the catalogue carries the
//! parallax anyway.
//!
//! **Aberration.** The Earth is moving at thirty kilometres a second, so
//! everything is displaced up to 20.5 arcseconds toward where the Earth is
//! going. Larger than parallax for every star, and the constant-of-aberration
//! approximation used here drops the orbit's eccentricity, worth 0.34
//! arcseconds at worst.

use crate::coordinates::Equatorial;
use crate::frame::{to_date, true_obliquity};
use crate::math::{acos, cos, sin, wrap360};
use crate::sun::Sun;
use crate::time::{Instant, J2000};

/// Arcseconds in one radian, which is also astronomical units in one parsec.
const ARCSECONDS_PER_RADIAN: f64 = 206_264.806_247_096_36;

/// Astronomical units travelled in a Julian year at one kilometre a second.
const AU_PER_YEAR_PER_KM_S: f64 = 0.210_945_021_053_5;

/// Days in a Julian year, the unit proper motion is quoted per.
const DAYS_PER_YEAR: f64 = 365.25;

/// The constant of aberration, in arcseconds.
const ABERRATION: f64 = 20.495_52;

/// One row of a star catalogue: where a star was at the catalogue epoch and
/// how fast it is going.
///
/// Angles are degrees, proper motions are milliarcseconds a year, parallax is
/// milliarcseconds and radial velocity is kilometres a second with positive
/// receding. [`proper_motion_ra`](Self::proper_motion_ra) already carries the
/// `cos(declination)` factor, which is how every modern catalogue quotes it and
/// is the one convention worth stating twice, because the other one is wrong by
/// a factor of eighty at Polaris.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Star {
    /// The star's common name.
    pub name: &'static str,
    /// The Hipparcos catalogue number, which is what to look the row up under.
    pub hip: u32,
    /// Right ascension in degrees, ICRS, at [`Star::EPOCH`].
    pub right_ascension: f64,
    /// Declination in degrees, ICRS, at [`Star::EPOCH`].
    pub declination: f64,
    /// Proper motion in right ascension, milliarcseconds a year, **including**
    /// the `cos(declination)` factor.
    pub proper_motion_ra: f64,
    /// Proper motion in declination, milliarcseconds a year.
    pub proper_motion_dec: f64,
    /// Trigonometric parallax, in milliarcseconds.
    pub parallax: f64,
    /// Radial velocity in kilometres a second, positive receding.
    pub radial_velocity: f64,
    /// Johnson `V` magnitude.
    pub magnitude: f64,
    /// The `B - V` colour index. Negative is blue-white, above 1.4 is deep
    /// orange; feeding it to a chromaticity is the renderer's business and not
    /// this crate's.
    pub colour_index: f64,
}

impl Star {
    /// The Julian day, on the Terrestrial Time scale, that the catalogue
    /// positions are given for: J2000.0.
    pub const EPOCH: f64 = J2000;

    /// The parallax, in milliarcseconds, above which the radial velocity is
    /// carried through the rollback.
    ///
    /// Below it, the star is far enough that the change in distance over two
    /// centuries cannot bend its path measurably and the rollback is a
    /// straight line in space with the distance held. Above it -- the couple of
    /// hundred nearest stars -- the perspective term is real: Barnard's Star is
    /// approaching at 110 kilometres a second, which over 211 years changes its
    /// distance by nearly a hundredth and its apparent proper motion with it.
    pub const NEAR_PARALLAX: f64 = 50.0;

    /// This star's catalogue row rolled to another epoch.
    ///
    /// Position, parallax, proper motion and radial velocity all move, because
    /// they are six numbers describing one straight line through space and
    /// moving along it changes all six. This is SOFA's `iauStarpm` without its
    /// light-deflection and light-time terms, neither of which reaches a
    /// milliarcsecond for a star.
    #[must_use]
    pub fn propagated(&self, instant: Instant) -> Self {
        let parallax_arcseconds = self.parallax / 1_000.0;
        if parallax_arcseconds <= 0.0 {
            return *self;
        }
        let distance = ARCSECONDS_PER_RADIAN / parallax_arcseconds;
        let years = (instant.terrestrial() - Self::EPOCH) / DAYS_PER_YEAR;

        let (east, north, radial) = self.basis();
        let radial_speed = if self.parallax > Self::NEAR_PARALLAX {
            self.radial_velocity * AU_PER_YEAR_PER_KM_S
        } else {
            0.0
        };
        let east_speed = distance * self.proper_motion_ra / 1_000.0 / ARCSECONDS_PER_RADIAN;
        let north_speed = distance * self.proper_motion_dec / 1_000.0 / ARCSECONDS_PER_RADIAN;

        let mut moved = [0.0; 3];
        let mut velocity = [0.0; 3];
        for axis in 0..3 {
            velocity[axis] =
                east_speed * east[axis] + north_speed * north[axis] + radial_speed * radial[axis];
            moved[axis] = distance * radial[axis] + velocity[axis] * years;
        }

        let range = libm::sqrt(moved[0] * moved[0] + moved[1] * moved[1] + moved[2] * moved[2]);
        let arrived = Equatorial::from_unit(moved);
        let mut rolled = *self;
        rolled.right_ascension = arrived.right_ascension;
        rolled.declination = arrived.declination;
        rolled.parallax = ARCSECONDS_PER_RADIAN / range * 1_000.0;

        // The velocity is unchanged in space, so the new proper motions are
        // just the same vector resolved on the new tangent plane and divided
        // by the new distance.
        let (east, north, radial) = rolled.basis();
        let project =
            |axis: [f64; 3]| velocity[0] * axis[0] + velocity[1] * axis[1] + velocity[2] * axis[2];
        rolled.proper_motion_ra = project(east) / range * ARCSECONDS_PER_RADIAN * 1_000.0;
        rolled.proper_motion_dec = project(north) / range * ARCSECONDS_PER_RADIAN * 1_000.0;
        if self.parallax > Self::NEAR_PARALLAX {
            rolled.radial_velocity = project(radial) / AU_PER_YEAR_PER_KM_S;
        }
        rolled
    }

    /// Where this star was seen from the Earth at a moment: apparent, referred
    /// to the true equator and equinox of date.
    ///
    /// All four corrections the module documentation lists, in that order.
    #[must_use]
    pub fn apparent(&self, instant: Instant) -> Equatorial {
        let rolled = self.propagated(instant);
        let direction = to_date(
            Equatorial::new(rolled.right_ascension, rolled.declination).to_unit(),
            instant.centuries(),
        );

        let sun = Sun::at(instant);
        let obliquity = true_obliquity(instant.centuries());
        // The Earth's heliocentric direction is the sun's geocentric direction
        // reversed, and its velocity is a quarter turn ahead of that in the
        // ecliptic plane.
        let earth = ecliptic_direction(sun.ecliptic_longitude + 180.0, obliquity);
        let heading = ecliptic_direction(sun.ecliptic_longitude - 90.0, obliquity);

        let parallax = rolled.parallax / 1_000.0 / ARCSECONDS_PER_RADIAN * sun.distance;
        let aberration = ABERRATION / ARCSECONDS_PER_RADIAN;
        let mut shifted = [0.0; 3];
        for axis in 0..3 {
            shifted[axis] = direction[axis] - parallax * earth[axis] + aberration * heading[axis];
        }
        Equatorial::from_unit(shifted)
    }

    /// The east, north and radial unit vectors at this star's catalogue
    /// position: the tangent plane proper motion is quoted on, plus the line of
    /// sight.
    fn basis(&self) -> ([f64; 3], [f64; 3], [f64; 3]) {
        let (sin_ra, cos_ra) = (sin(self.right_ascension), cos(self.right_ascension));
        let (sin_dec, cos_dec) = (sin(self.declination), cos(self.declination));
        (
            [-sin_ra, cos_ra, 0.0],
            [-sin_dec * cos_ra, -sin_dec * sin_ra, cos_dec],
            [cos_dec * cos_ra, cos_dec * sin_ra, sin_dec],
        )
    }

    /// The angle this star has moved across the sky between two epochs, in
    /// degrees.
    ///
    /// The quantity a catalogued proper motion is a claim about, measured
    /// rather than assumed: it comes from the two propagated positions and not
    /// from multiplying the row by a number of years.
    #[must_use]
    pub fn travelled(&self, from: Instant, to: Instant) -> f64 {
        let start = self.propagated(from);
        let end = self.propagated(to);
        let first = Equatorial::new(start.right_ascension, start.declination).to_unit();
        let second = Equatorial::new(end.right_ascension, end.declination).to_unit();
        acos(first[0] * second[0] + first[1] * second[1] + first[2] * second[2])
    }
}

/// A unit vector at an ecliptic longitude on the ecliptic, in equatorial
/// coordinates.
fn ecliptic_direction(longitude: f64, obliquity: f64) -> [f64; 3] {
    let folded = wrap360(longitude);
    [
        cos(folded),
        sin(folded) * cos(obliquity),
        sin(folded) * sin(obliquity),
    ]
}
