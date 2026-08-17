//! Obliquity, nutation, precession and sidereal time: the frame everything
//! else is expressed in.
//!
//! Precession is the big term and the one nobody remembers. General precession
//! is about 50.3 arcseconds a year, so the celestial frame has turned nearly
//! three degrees since J2000 -- six solar diameters -- and a 1789 sky drawn from
//! J2000 catalogue positions has every constellation in the wrong place by
//! that much. Proper motion, the famous correction, is a hundredth of it.
//!
//! The precession here is the **IAU 2006** model in its Fukushima-Williams
//! form: the four angles of Capitaine, Wallace and Chapront (2003),
//! *Astronomy and Astrophysics* 412, 567, as tabulated for SOFA's `iauPfw06`,
//! composed into a rotation exactly as `iauFw2m` composes it. Nutation is the
//! abridged series of Meeus, *Astronomical Algorithms*, 2nd ed., equation
//! 22.1, good to 0.5 arcsecond in longitude -- three orders of magnitude below
//! anything a picture can show.

use crate::coordinates::Equatorial;
use crate::math::{ARCSECONDS_PER_DEGREE, cos, poly_arcseconds, sin, wrap360};
use crate::time::{DAYS_PER_CENTURY, Instant, J2000};

/// The wobble of the Earth's axis on top of its precessional cone, as the two
/// corrections it makes to the frame.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Nutation {
    /// Nutation in longitude, in degrees. About 17 arcseconds at most.
    pub longitude: f64,
    /// Nutation in obliquity, in degrees. About 9 arcseconds at most.
    pub obliquity: f64,
}

/// The abridged nutation series, Meeus equation 22.1.
#[must_use]
pub fn nutation(centuries: f64) -> Nutation {
    let node = wrap360(
        125.044_52
            + centuries * (-1_934.136_261 + centuries * (0.002_070_8 + centuries / 450_000.0)),
    );
    let sun = wrap360(280.466_5 + 36_000.769_8 * centuries);
    let moon = wrap360(218.316_5 + 481_267.881_3 * centuries);
    Nutation {
        longitude: (-17.20 * sin(node) - 1.32 * sin(2.0 * sun) - 0.23 * sin(2.0 * moon)
            + 0.21 * sin(2.0 * node))
            / ARCSECONDS_PER_DEGREE,
        obliquity: (9.20 * cos(node) + 0.57 * cos(2.0 * sun) + 0.10 * cos(2.0 * moon)
            - 0.09 * cos(2.0 * node))
            / ARCSECONDS_PER_DEGREE,
    }
}

/// The mean obliquity of the ecliptic, in degrees: IAU 2006's `epsilon_A`.
#[must_use]
pub fn mean_obliquity(centuries: f64) -> f64 {
    poly_arcseconds(
        &[
            84_381.406,
            -46.836_769,
            -0.000_183_1,
            0.002_003_40,
            -0.000_000_576,
            -0.000_000_043_4,
        ],
        centuries,
    )
}

/// The true obliquity, which is the mean one plus the nutation in it.
#[must_use]
pub fn true_obliquity(centuries: f64) -> f64 {
    mean_obliquity(centuries) + nutation(centuries).obliquity
}

/// An ecliptic longitude and latitude turned into a direction on the equator.
///
/// Both arguments and the obliquity are in degrees, and the frame the answer
/// is in is whichever frame the ecliptic coordinates were: feed it apparent
/// longitude and the true obliquity and the answer is apparent, of date.
#[must_use]
pub fn from_ecliptic(longitude: f64, latitude: f64, obliquity: f64) -> Equatorial {
    let (sin_latitude, cos_latitude) = (sin(latitude), cos(latitude));
    let (sin_longitude, cos_longitude) = (sin(longitude), cos(longitude));
    let (sin_obliquity, cos_obliquity) = (sin(obliquity), cos(obliquity));
    Equatorial::from_unit([
        cos_latitude * cos_longitude,
        cos_latitude * sin_longitude * cos_obliquity - sin_latitude * sin_obliquity,
        cos_latitude * sin_longitude * sin_obliquity + sin_latitude * cos_obliquity,
    ])
}

/// Greenwich mean sidereal time in degrees, Meeus equation 12.4.
///
/// The argument is Universal Time, because sidereal time *is* the Earth's
/// rotation angle and has nothing to say about a uniform timescale.
#[must_use]
pub fn mean_sidereal(instant: Instant) -> f64 {
    let days = instant.universal() - J2000;
    let centuries = days / DAYS_PER_CENTURY;
    wrap360(
        280.460_618_37 + 360.985_647_366_29 * days + centuries * centuries * 0.000_387_933
            - centuries * centuries * centuries / 38_710_000.0,
    )
}

/// Greenwich apparent sidereal time in degrees: the mean value plus the
/// equation of the equinoxes.
#[must_use]
pub fn apparent_sidereal(instant: Instant) -> f64 {
    let centuries = instant.centuries();
    wrap360(mean_sidereal(instant) + nutation(centuries).longitude * cos(true_obliquity(centuries)))
}

/// A direction in the ICRS carried to the true equator and equinox of date.
///
/// Four frame rotations, and each one is a frame a reader can name. `gamma`
/// about the ICRS pole puts the `x` axis on the node where the ecliptic of date
/// crosses the ICRS equator. `phi` about that axis is the tilt onto the
/// ecliptic of date. `-(psi + dpsi)` about the ecliptic pole is the precession
/// and nutation in longitude themselves, which is the term worth three degrees.
/// `-(epsilon + depsilon)` tilts back down to the equator of date.
///
/// At `centuries == 0.0` it is not quite the identity, and should not be -- the
/// residue is the frame bias, the 0.02 arcsecond by which the ICRS pole is not
/// the mean pole of J2000.
#[must_use]
pub fn to_date(vector: [f64; 3], centuries: f64) -> [f64; 3] {
    let shift = nutation(centuries);
    let gamma = poly_arcseconds(
        &[
            -0.052_928,
            10.556_378,
            0.493_204_4,
            -0.000_312_38,
            -0.000_002_788,
            0.000_000_026_0,
        ],
        centuries,
    );
    let phi = poly_arcseconds(
        &[
            84_381.412_819,
            -46.811_016,
            0.051_126_8,
            0.000_532_89,
            -0.000_000_440,
            -0.000_000_017_6,
        ],
        centuries,
    );
    let psi = poly_arcseconds(
        &[
            -0.041_775,
            5_038.481_484,
            1.558_417_5,
            -0.000_185_22,
            -0.000_026_452,
            -0.000_000_014_8,
        ],
        centuries,
    );
    let epsilon = mean_obliquity(centuries);

    let turned = rotate_z(vector, gamma);
    let turned = rotate_x(turned, phi);
    let turned = rotate_z(turned, -(psi + shift.longitude));
    rotate_x(turned, -(epsilon + shift.obliquity))
}

/// A frame rotation about the `z` axis, by an angle in degrees.
fn rotate_z(vector: [f64; 3], angle: f64) -> [f64; 3] {
    let (sine, cosine) = (sin(angle), cos(angle));
    [
        cosine * vector[0] + sine * vector[1],
        cosine * vector[1] - sine * vector[0],
        vector[2],
    ]
}

/// A frame rotation about the `x` axis, by an angle in degrees.
fn rotate_x(vector: [f64; 3], angle: f64) -> [f64; 3] {
    let (sine, cosine) = (sin(angle), cos(angle));
    [
        vector[0],
        cosine * vector[1] + sine * vector[2],
        cosine * vector[2] - sine * vector[1],
    ]
}
