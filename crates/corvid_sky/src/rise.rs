//! Rise, transit and set, by scanning the day and then bisecting.
//!
//! Meeus chapter 15 does this with an interpolation from three positions,
//! which is efficient and fails quietly on the moon: the moon's declination
//! moves five degrees a day, so the three-point fit is a fifth-order problem
//! solved as a second-order one. This crate has a whole ephemeris and a
//! budget, so it scans the day at ten-minute steps and bisects each crossing
//! against the ephemeris itself. Slower, exactly right, and it handles a body
//! that rises twice in a day or not at all without a special case for either.

use crate::coordinates::Equatorial;
use crate::moon::Moon;
use crate::observer::Observer;
use crate::star::Star;
use crate::sun::Sun;
use crate::time::Instant;

/// Steps the day is scanned in. Ten minutes: shorter than the narrowest
/// interval between two crossings any real body produces, which is what makes
/// one sign change per interval a safe assumption.
const STEPS: u16 = 144;

/// Bisections per crossing. Forty halvings of a ten-minute interval land
/// inside a nanosecond, so this converges to the limit of the `f64` Julian day
/// long before it runs out.
const BISECTIONS: usize = 40;

/// The standard horizon for a point source, in degrees: refraction alone.
const POINT_HORIZON: f64 = -0.566_7;

/// The standard horizon for the sun's upper limb, in degrees: refraction plus
/// the semidiameter.
const SOLAR_HORIZON: f64 = -0.833_3;

/// When a body crossed the horizon and when it was highest.
///
/// Every field is optional because every field can genuinely not happen. A
/// circumpolar star never rises and never sets and is up the whole time; a
/// star below the horizon does the same in reverse; and the moon, whose day is
/// fifty minutes longer than the Earth's, skips a rise or a set roughly once a
/// month.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct RiseSet {
    /// When the body crossed the horizon going up.
    pub rise: Option<Instant>,
    /// When the body crossed the observer's meridian, which is when it was
    /// highest. Present even when the body never rose.
    pub transit: Option<Instant>,
    /// When the body crossed the horizon going down.
    pub set: Option<Instant>,
}

impl RiseSet {
    /// Sunrise, local apparent noon and sunset on the Universal Time day
    /// containing `day`.
    ///
    /// The horizon is the standard `-0.8333` degrees: 34 arcminutes of refraction
    /// and 16 of the sun's own radius, because sunrise is the first glint of
    /// the upper limb and not the centre of the disc.
    #[must_use]
    pub fn sun(observer: &Observer, day: Instant) -> Self {
        Self::scan(observer, day, |instant| {
            (Sun::at(instant).equatorial, SOLAR_HORIZON)
        })
    }

    /// Moonrise, transit and moonset on the Universal Time day containing
    /// `day`.
    ///
    /// **Topocentric**, and the horizon moves with the moon's distance: Meeus
    /// gives it as `0.727_5 * parallax - 34 arcminutes`, which is the upper limb
    /// of a body a degree wide seen from the surface rather than the centre of
    /// one seen from the Earth's core. Taking the geocentric direction instead
    /// puts moonset out by up to half an hour.
    #[must_use]
    pub fn moon(observer: &Observer, day: Instant) -> Self {
        Self::scan(observer, day, |instant| {
            let moon = Moon::at(instant);
            (
                observer.topocentric(instant, moon.equatorial, moon.parallax),
                0.727_5 * moon.parallax + POINT_HORIZON,
            )
        })
    }

    /// When a star rose, transited and set on the Universal Time day
    /// containing `day`.
    #[must_use]
    pub fn star(observer: &Observer, day: Instant, star: &Star) -> Self {
        Self::scan(observer, day, |instant| {
            (star.apparent(instant), POINT_HORIZON)
        })
    }

    /// When the sun passed a given number of degrees **below** the horizon:
    /// the twilights.
    ///
    /// Six is civil twilight, twelve nautical, eighteen astronomical. The
    /// `rise` field is the morning crossing and `set` the evening one, keeping
    /// the same sense as [`sun`](Self::sun) -- `rise` is the one where the sun
    /// is on its way up.
    #[must_use]
    pub fn depression(observer: &Observer, day: Instant, degrees: f64) -> Self {
        Self::scan(observer, day, |instant| {
            (Sun::at(instant).equatorial, -degrees)
        })
    }

    /// The scan itself: sample the day, find the sign changes, bisect each.
    fn scan(
        observer: &Observer,
        day: Instant,
        mut ephemeris: impl FnMut(Instant) -> (Equatorial, f64),
    ) -> Self {
        let midnight = day.midnight();
        let step = 1.0 / f64::from(STEPS);
        // Altitude above the target horizon, and the meridian angle: the hour
        // angle folded into +/-180, which crosses zero upward exactly at
        // transit.
        let mut altitude = |instant: Instant| {
            let (direction, horizon) = ephemeris(instant);
            observer.horizontal(instant, direction).altitude - horizon
        };

        let mut events = Self {
            rise: None,
            transit: None,
            set: None,
        };
        let mut previous_time = midnight;
        let mut previous = altitude(previous_time);
        for index in 1..=STEPS {
            let time = midnight.shift_days(f64::from(index) * step);
            let current = altitude(time);
            if previous < 0.0 && current >= 0.0 && events.rise.is_none() {
                events.rise = Some(bisect(previous_time, time, &mut altitude));
            } else if previous >= 0.0 && current < 0.0 && events.set.is_none() {
                events.set = Some(bisect(previous_time, time, &mut altitude));
            }
            previous_time = time;
            previous = current;
        }

        events.transit = transit(observer, midnight, step, &mut ephemeris);
        events
    }
}

/// The meridian crossing, found the same way on a different function.
fn transit(
    observer: &Observer,
    midnight: Instant,
    step: f64,
    ephemeris: &mut impl FnMut(Instant) -> (Equatorial, f64),
) -> Option<Instant> {
    let mut meridian = |instant: Instant| {
        let (direction, _) = ephemeris(instant);
        crate::math::wrap180(observer.hour_angle(instant, direction.right_ascension))
    };
    let mut previous_time = midnight;
    let mut previous = meridian(previous_time);
    for index in 1..=STEPS {
        let time = midnight.shift_days(f64::from(index) * step);
        let current = meridian(time);
        // The upward zero crossing, and not the wrap from +180 to -180, which
        // is also a sign change and is the anti-meridian.
        if previous < 0.0 && current >= 0.0 && current - previous < 180.0 {
            return Some(bisect(previous_time, time, &mut meridian));
        }
        previous_time = time;
        previous = current;
    }
    None
}

/// The moment inside a bracketing interval at which a rising function crosses
/// zero.
fn bisect(low: Instant, high: Instant, value: &mut impl FnMut(Instant) -> f64) -> Instant {
    let rising = value(high) > value(low);
    let (mut start, mut end) = (low.universal(), high.universal());
    for _ in 0..BISECTIONS {
        let middle = 0.5 * (start + end);
        let Ok(instant) = Instant::from_universal(middle) else {
            break;
        };
        if (value(instant) < 0.0) == rising {
            start = middle;
        } else {
            end = middle;
        }
    }
    Instant::from_universal(0.5 * (start + end)).unwrap_or(low)
}
