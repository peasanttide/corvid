//! Rise, set, refraction and the air between.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::float_cmp,
    reason = "the two exact comparisons here assert that one function answered the same bits twice, which is the claim; a tolerance would let a clamp that had stopped clamping through"
)]

use corvid_sky::{
    Atmosphere, Civil, Instant, Observer, RiseSet, Sky, Sun, Twilight, henyey_greenstein,
    rayleigh_phase,
};

/// A site on the Paris meridian, used because the whole of this crate's
/// reference material is for it. Nothing about it is in the crate itself.
fn site() -> Observer {
    Observer::new(48.8566, 2.3372, 35.0).unwrap()
}

#[test]
fn sunrise_and_sunset_bracket_local_noon() {
    // The property that catches a sign error, an hour-angle convention flipped,
    // or a transit found at the anti-meridian: whatever the date and whatever
    // the latitude, the sun comes up before it is highest and goes down after.
    // Four dates across a year, because the day length at 49 degrees north runs
    // from eight hours to sixteen and a fit that works in April can fail in
    // December.
    for (month, day) in [(1, 15), (4, 28), (7, 15), (10, 15)] {
        let noon = Instant::from_civil(Civil::new(1789, month, day, 12, 0, 0.0)).unwrap();
        let events = RiseSet::sun(&site(), noon);
        let rise = events.rise.unwrap();
        let transit = events.transit.unwrap();
        let set = events.set.unwrap();

        assert!(
            rise.universal() < transit.universal(),
            "{month}/{day}: sunrise {} is not before noon {}",
            rise.civil(),
            transit.civil()
        );
        assert!(
            transit.universal() < set.universal(),
            "{month}/{day}: noon {} is not before sunset {}",
            transit.civil(),
            set.civil()
        );

        // And the transit is local apparent noon on the sundial, to the
        // second. That is a joint statement about sidereal time, the equation
        // of time and the site's longitude: get any one of the three wrong and
        // this moves by minutes.
        let apparent = site().apparent_solar(transit);
        assert!(
            (apparent - 12.0).abs() < 1.0 / 3600.0,
            "{month}/{day}: transit at {apparent} on the apparent solar clock"
        );
    }
}

#[test]
fn the_twilights_come_in_order() {
    // Each deeper twilight ends later in the evening and starts earlier in the
    // morning, and all three sit outside sunset. The sun is one body moving one
    // way, so anything else means the depression angle is being applied with
    // the wrong sign somewhere.
    let day = Instant::from_civil(Civil::new(1789, 4, 28, 12, 0, 0.0)).unwrap();
    let sunset = RiseSet::sun(&site(), day).set.unwrap().universal();
    let civil = RiseSet::depression(&site(), day, 6.0)
        .set
        .unwrap()
        .universal();
    let nautical = RiseSet::depression(&site(), day, 12.0)
        .set
        .unwrap()
        .universal();
    let astronomical = RiseSet::depression(&site(), day, 18.0)
        .set
        .unwrap()
        .universal();

    assert!(sunset < civil, "civil dusk before sunset");
    assert!(civil < nautical, "nautical dusk before civil");
    assert!(nautical < astronomical, "astronomical dusk before nautical");

    // And the band the `Twilight` enum reports agrees with the crossings that
    // produced those times: a minute after each one, the sky is in the next
    // band down.
    for (moment, band) in [
        (sunset + 60.0 / 86400.0, Twilight::Civil),
        (civil + 60.0 / 86400.0, Twilight::Nautical),
        (nautical + 60.0 / 86400.0, Twilight::Astronomical),
        (astronomical + 60.0 / 86400.0, Twilight::Night),
    ] {
        let instant = Instant::from_universal(moment).unwrap();
        assert_eq!(Sky::new(instant, site()).twilight(), band);
    }
}

#[test]
fn refraction_is_the_standard_thirty_four_arcminutes() {
    // The number every table of sunrise is built on, and the one worth stating
    // the right way round: 34 arcminutes is the refraction at a *true* altitude
    // of -34 arcminutes, which is to say a body whose true altitude is that far
    // under the horizon is seen exactly on it.
    let air = site();
    let horizon = air.refraction(-34.0 / 60.0) * 60.0;
    assert!(
        (horizon - 34.0).abs() < 0.5,
        "refraction at the horizon was {horizon} arcminutes"
    );

    // Refraction falls off fast and is not quite zero overhead.
    assert!(air.refraction(45.0) * 60.0 < 1.1);
    assert!(air.refraction(90.0).abs() < 1e-4);
    assert!(air.refraction(0.0) > air.refraction(10.0));

    // And colder, denser air lifts the horizon further, which is the whole
    // reason the pressure and temperature are on the observer at all.
    let cold = site().with_air(1030.0, 0.0);
    assert!(cold.refraction(0.0) > air.refraction(0.0));
}

#[test]
fn a_circumpolar_star_never_sets_and_a_southern_one_never_rises() {
    // Both `None` cases, which a rise-and-set routine that assumes a crossing
    // exists will either miss or invent.
    let day = Instant::from_civil(Civil::new(1789, 4, 28, 12, 0, 0.0)).unwrap();
    let north = corvid_sky::BRIGHT_STARS
        .iter()
        .find(|row| row.name == "Polaris")
        .unwrap();
    let south = corvid_sky::BRIGHT_STARS
        .iter()
        .find(|row| row.name == "Canopus")
        .unwrap();

    let circumpolar = RiseSet::star(&site(), day, north);
    assert!(circumpolar.rise.is_none() && circumpolar.set.is_none());
    assert!(
        circumpolar.transit.is_some(),
        "it still crosses the meridian"
    );

    let invisible = RiseSet::star(&site(), day, south);
    assert!(invisible.rise.is_none() && invisible.set.is_none());
    assert!(Sky::new(day, site()).star_position(south).altitude < 0.0);
}

#[test]
fn the_equation_of_time_stays_inside_a_quarter_of_an_hour() {
    // Its envelope over a year: about -14 minutes in mid-February and +16 in
    // early November, crossing zero four times. A test on the envelope and on
    // the crossings, because a sign error would keep the envelope and lose the
    // crossings.
    let start = Instant::from_civil(Civil::new(1789, 1, 1, 12, 0, 0.0)).unwrap();
    let mut crossings = 0;
    let mut previous = Sun::equation_of_time(start);
    let (mut lowest, mut highest) = (previous, previous);
    for day in 1..365 {
        let value = Sun::equation_of_time(start.shift_days(f64::from(day)));
        if (value < 0.0) != (previous < 0.0) {
            crossings += 1;
        }
        lowest = f64::min(lowest, value);
        highest = f64::max(highest, value);
        previous = value;
    }
    assert!((-15.0..-13.0).contains(&lowest), "minimum {lowest} minutes");
    assert!((15.0..17.0).contains(&highest), "maximum {highest} minutes");
    assert_eq!(crossings, 4, "the equation of time crosses zero four times");
}

#[test]
fn the_atmosphere_hands_over_usable_numbers() {
    let air = Atmosphere::EARTH;

    // Air mass: one looking up, 38 looking along the ground. The flat-slab
    // `1 / sin(altitude)` gives infinity for the second, which is the error
    // this formula exists to avoid.
    assert!((Atmosphere::air_mass(90.0) - 1.0).abs() < 0.001);
    assert!((Atmosphere::air_mass(0.0) - 37.92).abs() < 0.1);
    assert_eq!(Atmosphere::air_mass(-10.0), Atmosphere::air_mass(0.0));

    // Blue is scattered several times as strongly as red, which is the whole
    // reason the sky is one colour and the sunset is another.
    let depth = air.zenith_optical_depth();
    assert!(
        depth[2] > 3.0 * depth[0],
        "blue {} red {}",
        depth[2],
        depth[0]
    );
    let low = air.transmittance(2.0);
    assert!(
        low[0] > low[2],
        "red must survive the long path better than blue"
    );

    // Extinction in magnitudes: a fifth of a magnitude overhead in clean air,
    // and enough at the horizon to take a first-magnitude star out of sight.
    assert!((0.1..0.3).contains(&air.extinction(90.0)));
    assert!(air.extinction(0.0) > 4.0);

    // Aerosol, which is the knob a burning building turns. Mie scattering is
    // grey, so a plume cuts every channel by the *same* factor -- it darkens the
    // sky without changing its colour, where Rayleigh cannot. That is the
    // property, and the factor is what a smoke column is worth.
    let smoke = air.with_aerosol(30.0);
    let clean = air.transmittance(30.0);
    let dimmed = smoke.transmittance(30.0);
    assert!(
        dimmed[1] < clean[1] * 0.7,
        "smoke cut only to {}",
        dimmed[1] / clean[1]
    );
    let factor = dimmed[0] / clean[0];
    assert!((dimmed[1] / clean[1] - factor).abs() < 1e-12);
    assert!((dimmed[2] / clean[2] - factor).abs() < 1e-12);

    // The two phase functions, each integrating to one over the sphere. The
    // sum over a uniform sampling in cos(theta) times 4 pi is that integral,
    // and it is the property a phase function that has lost its normalisation
    // fails.
    for asymmetry in [0.0, 0.5, air.mie_asymmetry] {
        let mut total = 0.0;
        for step in 0..2000 {
            let cosine = (f64::from(step) + 0.5) / 1000.0 - 1.0;
            total += henyey_greenstein(cosine, asymmetry) * 2.0 * core::f64::consts::PI / 1000.0;
        }
        assert!(
            (total - 1.0).abs() < 0.01,
            "Henyey-Greenstein at g = {asymmetry} integrates to {total}"
        );
    }
    let mut total = 0.0;
    for step in 0..2000 {
        let cosine = (f64::from(step) + 0.5) / 1000.0 - 1.0;
        total += rayleigh_phase(cosine) * 2.0 * core::f64::consts::PI / 1000.0;
    }
    assert!(
        (total - 1.0).abs() < 0.001,
        "Rayleigh integrates to {total}"
    );
    assert!(rayleigh_phase(1.0) > rayleigh_phase(0.0));
}
