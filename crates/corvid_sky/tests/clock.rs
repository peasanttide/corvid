//! The calendar, the Julian day, and the two timescales.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_sky::{Civil, Instant, SkyError};

#[test]
fn the_julian_day_matches_the_published_anchors() {
    // Four dates every ephemeris agrees on, and the third and fourth are the
    // ones that catch a calendar written as though Gregory never happened: the
    // last day of the Julian calendar and the first of the Gregorian are
    // consecutive Julian days ten calendar days apart.
    for (civil, day) in [
        (Civil::new(2000, 1, 1, 12, 0, 0.0), 2_451_545.0),
        (Civil::new(1789, 4, 25, 0, 0, 0.0), 2_374_593.5),
        (Civil::new(1582, 10, 4, 0, 0, 0.0), 2_299_159.5),
        (Civil::new(1582, 10, 15, 0, 0, 0.0), 2_299_160.5),
        (Civil::new(-4712, 1, 1, 12, 0, 0.0), 0.0),
    ] {
        let instant = Instant::from_civil(civil).unwrap();
        assert!(
            (instant.universal() - day).abs() < 1e-9,
            "{civil} came out as Julian day {}, not {day}",
            instant.universal()
        );
    }
}

#[test]
fn the_calendar_round_trips() {
    // Every hour of a fortnight across the level's own dates, and a scatter of
    // whole centuries, because the reverse conversion has a century branch of
    // its own that a fortnight would never reach.
    let start = Instant::from_civil(Civil::new(1789, 4, 20, 0, 0, 0.0)).unwrap();
    for step in 0..336 {
        let moment = start.shift_seconds(f64::from(step) * 3600.0);
        let back = Instant::from_civil(moment.civil()).unwrap();
        assert!(
            (back.universal() - moment.universal()).abs() * 86400.0 < 0.002,
            "{} did not survive a round trip",
            moment.civil()
        );
    }
    for year in [-100, 1, 500, 1000, 1500, 1582, 1600, 1900, 2000, 2400] {
        let civil = Civil::new(year, 3, 1, 6, 30, 15.5);
        let moment = Instant::from_civil(civil).unwrap();
        let back = moment.civil();
        assert_eq!((back.year, back.month, back.day), (year, 3, 1), "{civil}");
        assert_eq!((back.hour, back.minute), (6, 30));
        assert!((back.second - 15.5).abs() < 0.002, "{back}");
    }
}

#[test]
fn a_field_out_of_range_is_an_error_and_says_which() {
    // Not a panic, and not a silently normalised date: a caller reading a
    // scanned page has to be told which field it misread.
    assert_eq!(
        Instant::from_civil(Civil::new(1789, 13, 1, 0, 0, 0.0)),
        Err(SkyError::Calendar { field: "month" })
    );
    assert_eq!(
        Instant::from_civil(Civil::new(1789, 4, 32, 0, 0, 0.0)),
        Err(SkyError::Calendar { field: "day" })
    );
    assert_eq!(
        Instant::from_civil(Civil::new(1789, 4, 28, 24, 0, 0.0)),
        Err(SkyError::Calendar { field: "hour" })
    );
    assert_eq!(
        Instant::from_civil(Civil::new(1789, 4, 28, 0, 60, 0.0)),
        Err(SkyError::Calendar { field: "minute" })
    );
    assert_eq!(
        Instant::from_civil(Civil::new(1789, 4, 28, 0, 0, f64::NAN)),
        Err(SkyError::Calendar { field: "second" })
    );
    assert_eq!(Instant::from_universal(f64::NAN), Err(SkyError::NotFinite));
}

#[test]
fn the_two_timescales_are_consistent_in_both_directions() {
    // Building a moment from Terrestrial Time and from Universal Time has to
    // land in the same place, because delta-T is a function of the date and the
    // date is nearly the same on both scales.
    let from_universal = Instant::from_civil(Civil::new(1789, 4, 28, 21, 0, 0.0)).unwrap();
    let from_terrestrial = Instant::from_terrestrial(from_universal.terrestrial()).unwrap();
    assert!(
        (from_terrestrial.universal() - from_universal.universal()).abs() * 86400.0 < 1e-6,
        "the two constructors disagree"
    );

    // Terrestrial Time is ahead of Universal Time everywhere in the historical
    // record, and by more the further back you go, because the Earth's rotation
    // has been slowing.
    let ancient = Instant::from_civil(Civil::new(1000, 1, 1, 0, 0, 0.0)).unwrap();
    assert!(ancient.delta_t() > from_universal.delta_t());
    assert!(from_universal.terrestrial() > from_universal.universal());
}

#[test]
fn stepping_a_day_at_a_time_lands_where_stepping_a_year_does() {
    // `shift_days` re-evaluates delta-T, so accumulating small steps must not
    // drift against one large one. A tenth of a millisecond over a year is the
    // `f64` Julian day's own resolution and not an error in the arithmetic.
    let start = Instant::from_civil(Civil::new(1789, 1, 1, 0, 0, 0.0)).unwrap();
    let mut walked = start;
    for _ in 0..365 {
        walked = walked.shift_days(1.0);
    }
    let jumped = start.shift_days(365.0);
    assert!(
        (walked.universal() - jumped.universal()).abs() * 86400.0 < 1e-3,
        "{} against {}",
        walked.civil(),
        jumped.civil()
    );
}

#[test]
fn midnight_is_the_start_of_the_universal_day() {
    for hour in [0, 1, 11, 12, 13, 23] {
        let moment = Instant::from_civil(Civil::new(1789, 4, 28, hour, 30, 0.0)).unwrap();
        let midnight = moment.midnight().civil();
        assert_eq!(
            (midnight.year, midnight.month, midnight.day, midnight.hour),
            (1789, 4, 28, 0),
            "{hour}:30 rounded down to {midnight}"
        );
        assert_eq!(midnight.minute, 0);
    }
}
