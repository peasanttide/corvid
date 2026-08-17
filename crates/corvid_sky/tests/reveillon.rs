//! The night this crate exists for, computed rather than decided.
//!
//! A riot in a faubourg of a city on the Paris meridian ran until four in the
//! morning of 29 April 1789, and the two hours of demolition that ended it
//! happened in the dark. Whether there was a moon over it is a fact and not a
//! mood, and the answer changes what the scene is lit by: a moon is a key
//! light, and no moon means the only light in the last five hours is what the
//! crowd had already set on fire.
//!
//! So the answer is asserted here rather than written into a design document.
//! Nothing in `src` mentions this site, this date or this level; everything
//! below is a caller's arithmetic on a general ephemeris. If the crate stops
//! agreeing with the historical record, this file is where it says so.
//!
//! The site is 48.8566 N, 2.3372 E -- the Paris meridian -- at 35 metres. Times
//! are quoted in **local apparent solar time**, because in 1789 there were no
//! time zones and no railway to impose one, and every witness, church bell and
//! palace clock in the record is on the sundial.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_sky::{Civil, Instant, Moon, Observer, Phase, RiseSet, Sky, Twilight};

/// The site everything below is computed for.
fn faubourg() -> Observer {
    Observer::new(48.8566, 2.3372, 35.0).unwrap()
}

/// Noon, Universal Time, on 28 April 1789 -- the day whose events are wanted.
fn the_day() -> Instant {
    Instant::from_civil(Civil::new(1789, 4, 28, 12, 0, 0.0)).unwrap()
}

/// The moment at which the sundial in the faubourg read `hours` on the night of
/// the 28th, taking hours past midnight as belonging to the 29th.
fn sundial(hours: f64) -> Instant {
    let day = if hours >= 12.0 { 28 } else { 29 };
    let seed = Instant::from_civil(Civil::new(1789, 4, day, 12, 0, 0.0)).unwrap();
    faubourg().apparent_solar_near(seed, hours)
}

#[test]
fn the_new_moon_of_april_1789_fell_on_the_twenty_fifth() {
    // Everything about the light that night follows from this one date. Three
    // days before the riot means a thin waxing crescent that sets early, where
    // three days after would have meant a moon up until dawn.
    let found =
        Moon::new_moon_near(Instant::from_civil(Civil::new(1789, 4, 26, 0, 0, 0.0)).unwrap());
    let date = found.civil();
    assert_eq!(
        (date.year, date.month, date.day),
        (1789, 4, 25),
        "the new moon came out on {date}"
    );
    // Late morning, Universal Time, which is a little before eleven on the
    // Paris sundial.
    assert!(
        (9..=10).contains(&date.hour),
        "the new moon came out at {date}"
    );
}

#[test]
fn the_sun_set_a_little_after_seven_and_the_sky_was_dark_by_half_past_nine() {
    let site = faubourg();
    let sun = RiseSet::sun(&site, the_day());
    let sunrise = site.apparent_solar(sun.rise.unwrap());
    let sunset = site.apparent_solar(sun.set.unwrap());
    let civil = site.apparent_solar(RiseSet::depression(&site, the_day(), 6.0).set.unwrap());
    let astronomical =
        site.apparent_solar(RiseSet::depression(&site, the_day(), 18.0).set.unwrap());

    // 04:47, 19:14, 19:49 and 21:25 on the sundial. Sunrise and sunset bracket
    // noon, which is the sanity check; the rest is the schedule the level runs
    // on.
    assert!((sunrise - 4.780).abs() < 0.01, "sunrise at {sunrise}");
    assert!((sunset - 19.236).abs() < 0.01, "sunset at {sunset}");
    assert!(sunrise < 12.0 && 12.0 < sunset);
    assert!((civil - 19.824).abs() < 0.01, "civil dusk at {civil}");
    assert!(
        (astronomical - 21.423).abs() < 0.01,
        "astronomical dark at {astronomical}"
    );
}

#[test]
fn the_moon_was_a_three_day_crescent_and_a_tenth_lit() {
    let sunset = RiseSet::sun(&faubourg(), the_day()).set.unwrap();
    let moon = Moon::at(sunset);

    // Three and a third days past new, so a crescent, and the lit fraction is
    // the number that matters: an eighth of a disc that is itself a hundred
    // thousand times fainter than the sun.
    let age = sunset.universal() - Moon::new_moon_near(sunset).universal();
    assert!((age - 3.379).abs() < 0.01, "the moon was {age} days old");
    assert_eq!(moon.phase(), Phase::WaxingCrescent);
    assert!(
        (0.11..0.14).contains(&moon.illuminated_fraction),
        "the moon was {} lit",
        moon.illuminated_fraction
    );
    assert!(
        (moon.elongation - 40.82).abs() < 0.05,
        "elongation {}",
        moon.elongation
    );
}

#[test]
fn there_was_no_moon_over_the_faubourg_after_eleven() {
    // The load-bearing answer. Moonset is at 22:35 on the sundial -- three and a
    // third hours after the sun -- and from then until dawn the moon is under
    // the horizon and falling. Every hour from eleven at night to four in the
    // morning is checked, because "it set" and "it stayed set" are different
    // claims and a topocentric correction with the wrong sign satisfies the
    // first.
    let site = faubourg();
    let moonset = site.apparent_solar(RiseSet::moon(&site, the_day()).set.unwrap());
    assert!((moonset - 22.576).abs() < 0.02, "moonset at {moonset}");

    for hours in [23.0, 0.0, 1.0, 2.0, 3.0, 4.0] {
        let sky = Sky::new(sundial(hours), site);
        let moon = sky.moon_position();
        assert!(
            moon.altitude < 0.0,
            "at {hours} on the sundial the moon was {} degrees up",
            moon.altitude
        );
    }

    // And at Ferrieres' four in the morning it is not marginally down, it is
    // nineteen degrees down: two hours from rising, on the far side of the
    // night. Nothing about this is a close call.
    let four = Sky::new(sundial(4.0), site);
    assert!(
        (four.moon_position().altitude + 19.36).abs() < 0.1,
        "the moon was {} degrees up at four",
        four.moon_position().altitude
    );
}

#[test]
fn the_sky_was_beginning_to_lighten_by_four_in_the_morning() {
    // The other half of the same question, and the one that decides when the
    // level's last act stops being lit by fire. At four the sun is six and two
    // thirds degrees down -- nautical twilight, the first grey -- and by half
    // past four it is inside civil twilight and the faubourg can see itself.
    let site = faubourg();
    let four = Sky::new(sundial(4.0), site);
    assert_eq!(four.twilight(), Twilight::Nautical);
    assert!(
        (four.sun_position().altitude + 6.66).abs() < 0.1,
        "the sun was {} degrees up at four",
        four.sun_position().altitude
    );

    // Two in the morning, by contrast, is astronomically dark, and that is the
    // window with no moon and no dawn in it: the hours the fires are the whole
    // lighting rig.
    let two = Sky::new(sundial(2.0), site);
    assert_eq!(two.twilight(), Twilight::Night);
    assert!(two.moon_position().altitude < -18.0);
}

#[test]
fn the_stars_that_were_up_that_night_were_up() {
    // Not a claim about the historical record -- nobody wrote down what was
    // overhead -- but the shape of the sky the engraving has to draw, computed
    // once so that a change in the catalogue or the precession shows up as a
    // failure here rather than as a constellation quietly in the wrong place.
    //
    // At two in the morning, sundial time, Arcturus is high in the south-west
    // and Vega is climbing in the east; both are late-spring evening stars in
    // the northern sky and both are where they should be.
    let sky = Sky::new(sundial(2.0), faubourg());
    let find = |name: &str| {
        let row = corvid_sky::BRIGHT_STARS.iter().find(|row| row.name == name);
        assert!(row.is_some(), "{name} is not in the embedded catalogue");
        row.unwrap()
    };

    let arcturus = sky.star_position(find("Arcturus"));
    assert!(
        arcturus.altitude > 40.0,
        "Arcturus at {}",
        arcturus.altitude
    );
    assert!((180.0..300.0).contains(&arcturus.azimuth));

    let vega = sky.star_position(find("Vega"));
    assert!(vega.altitude > 20.0, "Vega at {}", vega.altitude);
    assert!((30.0..120.0).contains(&vega.azimuth));

    // And the counter-case, because a sky where everything is up is a sky with
    // a broken horizon: Sirius has been gone for hours by two in the morning in
    // late April.
    assert!(sky.star_position(find("Sirius")).altitude < 0.0);

    // How many first-magnitude stars a clear sky over the faubourg had: enough
    // to be a sky, few enough to be an engraving.
    let visible = corvid_sky::brighter_than(1.5)
        .filter(|row| sky.star_position(row).altitude > 0.0)
        .count();
    assert!((3..=10).contains(&visible), "{visible} bright stars up");
}
