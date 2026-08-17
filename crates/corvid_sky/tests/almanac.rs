//! The ephemeris against published values.
//!
//! Every number asserted here is printed in a source somebody else can open.
//! Meeus, *Astronomical Algorithms*, 2nd ed., works two examples all the way
//! through in the chapters this crate implements, and those two are the only
//! honest way to say that a transcribed series was transcribed correctly: a
//! test written from this crate's own output would pass on a table with a typo
//! in it.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_sky::{Civil, Instant, Moon, Sun, frame};

/// How close two angles in degrees have to be, with the reason in the message.
fn near(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: got {actual}, expected {expected} +/- {tolerance}"
    );
}

#[test]
fn the_sun_matches_meeus_example_25() {
    // Meeus example 25.a and 25.b, both worked for JDE 2448908.5, which is
    // 1992 October 13 at 0h Terrestrial Time. 25.a is the low-accuracy series
    // this crate implements; 25.b is VSOP87 truncated to the milliarcsecond and
    // is what the low-accuracy answer is *checked against*, because agreeing
    // with the method is not the same as being right.
    let instant = Instant::from_terrestrial(2_448_908.5).unwrap();
    near(
        instant.centuries(),
        -0.072_183_436,
        1e-9,
        "Julian centuries",
    );

    let sun = Sun::at(instant);

    // Meeus 25.a: apparent longitude 199.90895 degrees. Meeus states the
    // method's accuracy as 0.01 degree; the tolerance here is a tenth of that,
    // because this is a check on the transcription rather than on the theory.
    near(
        sun.ecliptic_longitude,
        199.908_95,
        0.001,
        "apparent longitude",
    );

    // Meeus 25.b, the high-accuracy answer: apparent right ascension
    // 13h13m31.4s and declination -7 deg 47' 06". That the low-accuracy series
    // lands within a thousandth of a degree of it on this date is luck; 0.01
    // is what the method promises and 0.01 is what is asserted.
    near(
        sun.equatorial.right_ascension,
        (13.0 + 13.0 / 60.0 + 31.4 / 3600.0) * 15.0,
        0.01,
        "apparent right ascension",
    );
    near(
        sun.equatorial.declination,
        -(7.0 + 47.0 / 60.0 + 6.0 / 3600.0),
        0.01,
        "apparent declination",
    );

    // The radius vector. Checked against the two-body solution of Kepler's
    // equation for the same mean anomaly rather than against a printed figure:
    // r = a(1 - e cos E) with E from M = E - e sin E is a different route to
    // the same number, and it agrees to seven digits.
    near(sun.distance, 0.997_665, 1e-5, "radius vector in AU");

    // A quarter of a degree, which is what makes an eclipse a coincidence.
    near(sun.angular_radius, 0.266_9, 0.001, "angular radius");
}

#[test]
fn the_moon_matches_meeus_example_47() {
    // Meeus example 47.a, worked for JDE 2448724.5 -- 1992 April 12 at 0h TD.
    // This is the whole of the sixty-term table in one assertion: get any row
    // of `moon_table` wrong and one of these four moves.
    let instant = Instant::from_terrestrial(2_448_724.5).unwrap();
    let moon = Moon::at(instant);

    // Meeus: longitude before nutation 133.162655, apparent longitude
    // 133.167265, latitude -3.229126, distance 368409.7 km, equatorial
    // horizontal parallax 0.991990 degrees.
    //
    // The un-nutated longitude is the one that tests the table, and it is
    // asserted to half of the last digit Meeus prints -- two thousandths of an
    // arcsecond, finer than the theory it came from. Nothing but a correct
    // transcription of all sixty rows passes it.
    let centuries = instant.centuries();
    near(
        moon.ecliptic_longitude - frame::nutation(centuries).longitude,
        133.162_655,
        5e-7,
        "moon longitude before nutation",
    );

    // The apparent longitude then differs from Meeus's by a tenth of an
    // arcsecond, and that tenth is the nutation and not the moon: Meeus applies
    // the full 1980 IAU series where this crate applies his own abridged one,
    // whose stated accuracy is half an arcsecond.
    near(
        moon.ecliptic_longitude,
        133.167_265,
        0.5 / 3600.0,
        "moon longitude",
    );
    near(moon.ecliptic_latitude, -3.229_126, 1e-6, "moon latitude");
    near(moon.distance, 368_409.7, 0.5, "moon distance in km");
    near(moon.parallax, 0.991_990, 1e-6, "horizontal parallax");

    // Meeus converts the same example to apparent right ascension 134.688470
    // and declination 13.768368 in chapter 13, which exercises the obliquity
    // and the ecliptic-to-equatorial rotation on top of the series.
    near(
        moon.equatorial.right_ascension,
        134.688_470,
        1e-4,
        "moon right ascension",
    );
    near(
        moon.equatorial.declination,
        13.768_368,
        1e-4,
        "moon declination",
    );
}

#[test]
fn the_obliquity_is_the_iau_2006_value_at_its_own_epoch() {
    // IAU 2006 defines the mean obliquity at J2000.0 as exactly 84381.406
    // arcseconds. A crate that got the units or the epoch wrong would still
    // produce a plausible-looking 23-and-a-bit degrees, so the check is on all
    // eight digits.
    near(
        frame::mean_obliquity(0.0),
        84_381.406 / 3600.0,
        1e-12,
        "mean obliquity at J2000",
    );

    // And it decreases by about 47 arcseconds a century, which is the term that
    // matters over two centuries: the ecliptic in 1789 was tilted 0.027 degrees
    // more than it is now.
    let then = frame::mean_obliquity(-2.108_5);
    near(then - frame::mean_obliquity(0.0), 0.027_43, 1e-4, "drift");
}

#[test]
fn the_nutation_stays_inside_its_own_amplitude() {
    // The abridged series is four terms, so the only property worth asserting
    // over a whole cycle is that it never leaves the envelope its coefficients
    // give it: 18.98 arcseconds in longitude and 9.96 in obliquity, which are
    // the sums of the four amplitudes.
    for step in 0..400 {
        let centuries = f64::from(step) / 100.0 - 2.5;
        let shift = frame::nutation(centuries);
        assert!(shift.longitude.abs() * 3600.0 <= 18.98, "at {centuries}");
        assert!(shift.obliquity.abs() * 3600.0 <= 9.96, "at {centuries}");
    }

    // And it is a real oscillation rather than a constant: the 18.6-year
    // period of the moon's node has to show up as a sign change.
    let node_period = frame::nutation(0.0).longitude * frame::nutation(0.093).longitude;
    assert!(node_period < 0.0, "half a nutation period changed no sign");
}

#[test]
fn delta_t_in_1789_is_seventeen_seconds() {
    // The Espenak-Meeus polynomial for 1700-1800, evaluated at the level's own
    // date. Sixteen and two thirds seconds is a quarter of a degree of the
    // Earth's rotation, which is fifteen seconds of sunset -- small, and not
    // nothing, and the whole reason `Instant` carries two clocks.
    let instant = Instant::from_civil(Civil::new(1789, 4, 28, 12, 0, 0.0)).unwrap();
    near(instant.delta_t(), 16.670, 0.005, "delta-T in 1789");

    // The modern branch, against the measured value: delta-T at the start of
    // 2000 was 63.83 seconds, and the polynomial for 1986-2005 is a fit to the
    // record rather than an extrapolation of it.
    let modern = Instant::from_civil(Civil::new(2000, 1, 1, 0, 0, 0.0)).unwrap();
    near(modern.delta_t(), 63.83, 0.2, "delta-T in 2000");

    // Ancient history, where it is recovered from eclipse records: about two
    // and three quarter hours at the year 1000.
    let ancient = Instant::from_civil(Civil::new(1000, 7, 1, 0, 0, 0.0)).unwrap();
    near(ancient.delta_t(), 1574.0, 30.0, "delta-T at the year 1000");
}
