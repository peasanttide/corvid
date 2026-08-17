//! Rolling the catalogue back two centuries.
//!
//! Three separable claims, tested separately because they fail separately.
//! Proper motion moves a star by the amount its row says it moves. Precession
//! moves the whole frame by three degrees, which is a hundred times more.
//! And the two together land where an implementation that shares no code with
//! this one lands.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_sky::{BRIGHT_STARS, Civil, Equatorial, Instant, Observer, Sky, Star, frame};

/// The row for a star, by name.
fn star(name: &str) -> &'static Star {
    let row = BRIGHT_STARS.iter().find(|row| row.name == name);
    assert!(row.is_some(), "{name} is not in the embedded catalogue");
    row.unwrap()
}

/// Arcseconds between two angles in degrees.
fn arcseconds(degrees: f64) -> f64 {
    degrees * 3600.0
}

#[test]
fn barnards_star_moves_by_the_catalogued_amount() {
    // The fastest proper motion known: 10362.394 milliarcseconds a year in
    // declination and -801.551 in right ascension, from van Leeuwen's Hipparcos
    // re-reduction. Over the 211.3 Julian years back to April 1789 that is
    // 2196 arcseconds -- six tenths of a degree, more than the moon is wide.
    let barnard = star("Barnard's Star");
    let epoch = Instant::from_terrestrial(Star::EPOCH).unwrap();
    let then = Instant::from_civil(Civil::new(1789, 4, 28, 0, 0, 0.0)).unwrap();
    let years = (then.terrestrial() - Star::EPOCH) / 365.25;

    let catalogued =
        barnard.proper_motion_ra.hypot(barnard.proper_motion_dec) / 1000.0 * years.abs();
    let travelled = arcseconds(barnard.travelled(epoch, then));

    assert!(
        (travelled - catalogued).abs() / catalogued < 0.02,
        "moved {travelled} arcseconds where the row says {catalogued}"
    );

    // Two percent, and the two percent is not slop: Barnard's Star is closing
    // at 110 kilometres a second, so in 1789 it was 1.3 percent further away
    // and its apparent motion was correspondingly slower. That is the
    // perspective term `Star::NEAR_PARALLAX` exists to switch on, and it shows
    // in the parallax coming back smaller as well as in the angle.
    let rolled = barnard.propagated(then);
    assert!(
        rolled.parallax < barnard.parallax,
        "an approaching star was not further away in the past"
    );
    assert!(
        (rolled.parallax - 539.94).abs() < 0.05,
        "parallax rolled back to {} mas, not 539.94",
        rolled.parallax
    );
    assert!(
        travelled < catalogued,
        "the perspective term must make the travel less than the linear estimate"
    );
}

#[test]
fn a_slow_star_barely_moves_and_a_fast_one_does() {
    // The spread the catalogue actually contains, over the same interval.
    // Deneb's proper motion is two and a half milliarcseconds a year, which is
    // half an arcsecond over two centuries and invisible; Kapteyn's Star is
    // three thousand times that.
    let epoch = Instant::from_terrestrial(Star::EPOCH).unwrap();
    let then = Instant::from_civil(Civil::new(1789, 4, 28, 0, 0, 0.0)).unwrap();

    let deneb = arcseconds(star("Deneb").travelled(epoch, then));
    let kapteyn = arcseconds(star("Kapteyn's Star").travelled(epoch, then));

    assert!(deneb < 1.0, "Deneb moved {deneb} arcseconds");
    assert!(kapteyn > 1500.0, "Kapteyn's Star moved {kapteyn}");
}

#[test]
fn precession_carries_the_equinox_and_dwarfs_proper_motion() {
    // The sharpest statement of what precession does, and the one that catches
    // a sign error: take the J2000 equinox itself -- the ICRS direction
    // `(1, 0, 0)` -- and ask where it is in the frame of 1789.
    //
    // The independent implementation described below, Meeus's rigorous
    // zeta-z-theta precession with the nutation applied afterwards, puts it at
    // right ascension 357.305585 and declination -1.171455 for 1789 April 28.
    // Both numbers are negative shifts, and both have to be: the equinox
    // regresses along the ecliptic with time, so a star's right ascension in
    // 1789 is nearly three degrees *less* than its right ascension now.
    let then = Instant::from_civil(Civil::new(1789, 4, 28, 0, 0, 0.0)).unwrap();
    let equinox = Equatorial::from_unit(frame::to_date([1.0, 0.0, 0.0], then.centuries()));
    assert!(
        (equinox.right_ascension - 357.305_585).abs() < 0.001,
        "the J2000 equinox landed at right ascension {}",
        equinox.right_ascension
    );
    assert!(
        (equinox.declination + 1.171_455).abs() < 0.001,
        "the J2000 equinox landed at declination {}",
        equinox.declination
    );

    // And the scale of it against the correction everybody remembers instead.
    // Deneb's proper motion is two and a half milliarcseconds a year, which is
    // half an arcsecond over two centuries; the frame moved ten thousand
    // arcseconds, which is four figures more.
    let epoch = Instant::from_terrestrial(Star::EPOCH).unwrap();
    let deneb = arcseconds(star("Deneb").travelled(epoch, then));
    let frame_shift = arcseconds(360.0 - equinox.right_ascension);
    assert!(
        frame_shift > 1000.0 * deneb,
        "precession {frame_shift} arcseconds should dwarf proper motion {deneb}"
    );
}

#[test]
fn arcturus_and_polaris_agree_with_an_independent_implementation() {
    // The reference values below were computed by a separate implementation
    // that shares no code with this crate and takes a different route to the
    // answer: Meeus's rigorous zeta-z-theta precession, which is IAU 1976 and
    // not IAU 2006, with the nutation applied as corrections to right
    // ascension and declination rather than as a rotation. Two models, two
    // formulations, one answer.
    //
    //   1789 April 29, 01:00 UT, at 48.8566 N 2.3372 E:
    //     Arcturus  RA 211.522456  Dec 20.284334  alt 55.927852  az 221.294532
    //     Polaris   RA  12.441884  Dec 88.180335  alt 47.511768  az   1.812643
    //
    // The acceptance figure for this is 0.02 degrees. The residual is under
    // 0.0005, and the residual *is* the difference between the two precession
    // models rather than anything either implementation got wrong.
    let site = Observer::new(48.8566, 2.3372, 35.0).unwrap();
    let moment = Instant::from_civil(Civil::new(1789, 4, 29, 1, 0, 0.0)).unwrap();
    let sky = Sky::new(moment, site);

    for (name, right_ascension, declination, altitude, azimuth) in [
        ("Arcturus", 211.522_456, 20.284_334, 55.927_852, 221.294_532),
        ("Polaris", 12.441_884, 88.180_335, 47.511_768, 1.812_643),
    ] {
        let row = star(name);
        let apparent = row.apparent(moment);
        let seen = sky.star_position(row);
        assert!(
            (apparent.right_ascension - right_ascension).abs() < 0.02,
            "{name} right ascension {} not {right_ascension}",
            apparent.right_ascension
        );
        assert!(
            (apparent.declination - declination).abs() < 0.02,
            "{name} declination {} not {declination}",
            apparent.declination
        );
        assert!(
            (seen.altitude - altitude).abs() < 0.02,
            "{name} altitude {} not {altitude}",
            seen.altitude
        );
        assert!(
            (seen.azimuth - azimuth).abs() < 0.02,
            "{name} azimuth {} not {azimuth}",
            seen.azimuth
        );
    }
}

#[test]
fn polaris_was_further_from_the_pole_in_1789() {
    // A check on the *sign* of precession, which is the one thing a rotation
    // can get wrong while still producing plausible numbers. Polaris has been
    // closing on the celestial pole for centuries and reaches it around 2100.
    // In 1789 it was 1.8 degrees off; today it is 0.74.
    let polaris = star("Polaris");
    let then = Instant::from_civil(Civil::new(1789, 4, 29, 1, 0, 0.0)).unwrap();
    let now = Instant::from_civil(Civil::new(2000, 1, 1, 12, 0, 0.0)).unwrap();

    let apart = |instant| 90.0 - polaris.apparent(instant).declination;
    assert!(
        apart(then) > apart(now),
        "Polaris was {} degrees from the pole in 1789 and {} in 2000",
        apart(then),
        apart(now)
    );
    assert!((apart(then) - 1.82).abs() < 0.05);
    assert!((apart(now) - 0.74).abs() < 0.05);
}

#[test]
fn the_catalogue_is_well_formed() {
    // The properties a hand-transcribed table can quietly violate: a duplicated
    // Hipparcos number from a copy-and-paste, a declination outside the sphere,
    // a negative parallax that would put a star behind the observer, and the
    // magnitude order `brighter_than` relies on being a prefix.
    let mut seen = std::collections::BTreeSet::new();
    for row in &BRIGHT_STARS {
        assert!(seen.insert(row.hip), "HIP {} appears twice", row.hip);
        assert!(!row.name.is_empty());
        assert!(
            (0.0..360.0).contains(&row.right_ascension),
            "{}: right ascension {}",
            row.name,
            row.right_ascension
        );
        assert!(
            (-90.0..=90.0).contains(&row.declination),
            "{}: declination {}",
            row.name,
            row.declination
        );
        assert!(
            row.parallax > 0.0,
            "{}: parallax {}",
            row.name,
            row.parallax
        );
        assert!(
            (-0.5..2.5).contains(&row.colour_index),
            "{}: colour index {}",
            row.name,
            row.colour_index
        );
    }
    assert_eq!(BRIGHT_STARS.len(), 89);
    assert!(
        BRIGHT_STARS
            .windows(2)
            .all(|two| two[0].magnitude <= two[1].magnitude)
    );

    // Every row rolls back without producing a direction that is not a
    // direction, which is what a zero or negative propagated distance would
    // give.
    let then = Instant::from_civil(Civil::new(1789, 4, 28, 0, 0, 0.0)).unwrap();
    for row in &BRIGHT_STARS {
        let apparent = row.apparent(then);
        assert!(
            apparent.right_ascension.is_finite() && apparent.declination.is_finite(),
            "{} did not roll back",
            row.name
        );
    }
}
