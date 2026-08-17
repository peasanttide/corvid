//! The phase, which is the half of the moon that decides whether there is
//! light.
//!
//! Position is checked in `tests/almanac.rs` against Meeus's worked example.
//! What is checked here is the other half: that a known new moon comes out new,
//! a known full moon comes out full, and the parallax that decides whether the
//! thing is above the rooftops at all is the degree it should be.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_sky::{Civil, Instant, Moon, Observer, Phase};

/// Minutes between two moments, signed.
fn minutes(later: Instant, earlier: Instant) -> f64 {
    (later.universal() - earlier.universal()) * 24.0 * 60.0
}

#[test]
fn a_known_new_moon_comes_out_new() {
    // The new moon of 2000 January 6, at 18:14 Universal Time. This one is in
    // every ephemeris there is, and it is the anchor everybody's lunation
    // number is counted from.
    let search = Instant::from_civil(Civil::new(2000, 1, 6, 0, 0, 0.0)).unwrap();
    let found = Moon::new_moon_near(search);
    let date = found.civil();

    assert_eq!((date.year, date.month, date.day), (2000, 1, 6));
    assert!(
        minutes(
            found,
            Instant::from_civil(Civil::new(2000, 1, 6, 18, 14, 0.0)).unwrap()
        )
        .abs()
            < 2.0,
        "new moon came out at {date}, not 2000-01-06 18:14"
    );

    // And the two answers a caller actually asks for.
    let moon = Moon::at(found);
    assert_eq!(moon.phase(), Phase::New);
    assert!(
        moon.illuminated_fraction < 0.001,
        "a new moon that is {} lit",
        moon.illuminated_fraction
    );
}

#[test]
fn a_known_full_moon_comes_out_full() {
    // The full moon of 2000 January 21 at 04:40 UT, which is the total lunar
    // eclipse of that night -- an eclipse happens *at* full moon, so the
    // eclipse's published time is an independent check on the phase search.
    let search = Instant::from_civil(Civil::new(2000, 1, 21, 0, 0, 0.0)).unwrap();
    let found = Moon::full_moon_near(search);
    let date = found.civil();

    assert_eq!((date.year, date.month, date.day), (2000, 1, 21));
    assert!(
        minutes(
            found,
            Instant::from_civil(Civil::new(2000, 1, 21, 4, 40, 0.0)).unwrap()
        )
        .abs()
            < 2.0,
        "full moon came out at {date}, not 2000-01-21 04:40"
    );

    let moon = Moon::at(found);
    assert_eq!(moon.phase(), Phase::Full);
    assert!(
        moon.illuminated_fraction > 0.999,
        "a full moon that is {} lit",
        moon.illuminated_fraction
    );
}

#[test]
fn the_phase_walks_the_whole_cycle_in_order() {
    // A synodic month sampled every three hours, from one new moon to the
    // next. Two properties, and between them they catch every way a phase can
    // be wired up backwards: the illuminated fraction rises to one at the
    // halfway point and falls again, and the eight names come out in their
    // declared order and each exactly once.
    let start =
        Moon::new_moon_near(Instant::from_civil(Civil::new(1789, 4, 26, 0, 0, 0.0)).unwrap());
    let mut names = Vec::new();
    let mut brightest = (0.0, 0.0);
    for step in 0..236 {
        let moment = start.shift_days(f64::from(step) * 0.125);
        let moon = Moon::at(moment);
        if names.last() != Some(&moon.phase()) {
            names.push(moon.phase());
        }
        if moon.illuminated_fraction > brightest.0 {
            brightest = (moon.illuminated_fraction, f64::from(step) * 0.125);
        }
    }

    assert_eq!(
        names,
        [
            Phase::New,
            Phase::WaxingCrescent,
            Phase::FirstQuarter,
            Phase::WaxingGibbous,
            Phase::Full,
            Phase::WaningGibbous,
            Phase::LastQuarter,
            Phase::WaningCrescent,
            Phase::New,
        ],
        "the eight sectors did not come round once in order"
    );
    assert!(brightest.0 > 0.99, "never reached full: {}", brightest.0);
    assert!(
        (brightest.1 - 14.75).abs() < 1.0,
        "full moon at {} days into the lunation, not half way",
        brightest.1
    );
}

#[test]
fn the_elongation_and_the_phase_angle_are_not_the_same_number() {
    // They are close enough to be confused and different enough to matter: the
    // phase angle is solved from the sun-earth-moon triangle and the elongation
    // is a difference of two longitudes, so they differ by the moon's ecliptic
    // latitude and by the parallax of the triangle itself.
    let moment = Instant::from_civil(Civil::new(1789, 4, 28, 21, 0, 0.0)).unwrap();
    let moon = Moon::at(moment);
    let difference = (180.0 - moon.elongation - moon.phase_angle).abs();
    assert!(
        difference > 0.001 && difference < 0.5,
        "elongation {} and phase angle {} differ by {difference}",
        moon.elongation,
        moon.phase_angle
    );

    // The illuminated fraction follows the phase angle and not the elongation,
    // and it is the phase angle it has to follow: a lit fraction taken from the
    // elongation instead would be a quarter of a percent out, which is more
    // than the half percent an ephemeris is checked to.
    let from_angle = f64::midpoint(1.0, moon.phase_angle.to_radians().cos());
    let from_elongation = f64::midpoint(1.0, (180.0 - moon.elongation).to_radians().cos());
    assert!((moon.illuminated_fraction - from_angle).abs() < 1e-12);
    assert!((moon.illuminated_fraction - from_elongation).abs() > 1e-6);
}

#[test]
fn topocentric_parallax_moves_the_moon_by_most_of_a_degree() {
    // The correction that decides whether a low crescent is above the rooftops
    // or already gone. It is largest for a moon on the horizon and vanishes for
    // one overhead, because the observer's displacement from the Earth's centre
    // is then along the line of sight.
    let site = Observer::new(48.8566, 2.3372, 35.0).unwrap();
    let mut largest: f64 = 0.0;
    let mut smallest = f64::INFINITY;
    let start = Instant::from_civil(Civil::new(1789, 4, 20, 0, 0, 0.0)).unwrap();
    for step in 0..300 {
        let moment = start.shift_days(f64::from(step) * 0.1);
        let moon = Moon::at(moment);
        let geocentric = site.horizontal(moment, moon.equatorial);
        let topocentric = site.horizontal(
            moment,
            site.topocentric(moment, moon.equatorial, moon.parallax),
        );
        let shift = geocentric.altitude - topocentric.altitude;
        if geocentric.altitude > 60.0 {
            smallest = smallest.min(shift);
        }
        if geocentric.altitude.abs() < 5.0 {
            largest = largest.max(shift);
        }
    }
    assert!(
        (0.85..1.02).contains(&largest),
        "parallax near the horizon was {largest} degrees, not most of one"
    );
    assert!(
        smallest < 0.5,
        "parallax high in the sky was {smallest} degrees, and should shrink"
    );
}
