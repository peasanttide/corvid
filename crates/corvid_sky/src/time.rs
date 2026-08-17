//! A civil date, a Julian day, and the two timescales an ephemeris keeps apart.

use crate::deltat;
use crate::error::SkyError;
use crate::math::whole;

/// Days in a Julian century, which is the unit every series here is fitted in.
pub(crate) const DAYS_PER_CENTURY: f64 = 36_525.0;

/// The Julian day number of the J2000.0 epoch, 2000 January 1 at 12h TT.
pub(crate) const J2000: f64 = 2_451_545.0;

/// Seconds in a day.
pub(crate) const SECONDS_PER_DAY: f64 = 86_400.0;

/// A date and a clock reading, in Universal Time.
///
/// The calendar is the historical one an almanac uses: Gregorian on and after
/// 1582 October 15, Julian before it. That is a deliberate choice rather than
/// an oversight -- a date read off an eighteenth-century page is Gregorian and a
/// date read off a fourteenth-century one is not, and a proleptic Gregorian
/// calendar would silently move the second by ten days.
///
/// ```
/// use corvid_sky::Civil;
///
/// // The night the Reveillon riot ran into, on the clock a witness read.
/// let civil = Civil::new(1789, 4, 29, 4, 0, 0.0);
/// assert_eq!(civil.year, 1789);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Civil {
    /// The year. Negative years count backwards through 1 BC as year 0, which
    /// is the astronomical convention and not the historian's.
    pub year: i32,
    /// The month, `1 ..= 12`.
    pub month: i32,
    /// The day of the month, `1 ..= 31`.
    pub day: i32,
    /// The hour, `0 ..= 23`.
    pub hour: i32,
    /// The minute, `0 ..= 59`.
    pub minute: i32,
    /// The second, `0.0 ..= 60.0` exclusive at the top.
    pub second: f64,
}

impl Civil {
    /// A civil date from its six fields, unchecked.
    ///
    /// Nothing is validated here; [`Instant::from_civil`] is where a field out
    /// of range becomes a [`SkyError`]. This is a struct literal with a shorter
    /// name.
    #[must_use]
    pub const fn new(year: i32, month: i32, day: i32, hour: i32, minute: i32, second: f64) -> Self {
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
        }
    }

    /// The fraction of a day past midnight the clock fields name.
    #[must_use]
    pub fn day_fraction(&self) -> f64 {
        (f64::from(self.hour) + (f64::from(self.minute) + self.second / 60.0) / 60.0) / 24.0
    }

    /// The decimal year the delta-T polynomials take as their argument.
    fn decimal_year(&self) -> f64 {
        f64::from(self.year) + (f64::from(self.month) - 0.5) / 12.0
    }

    /// Every field in the range its documentation gives it.
    fn check(&self) -> Result<(), SkyError> {
        let field = if !(1..=12).contains(&self.month) {
            "month"
        } else if !(1..=31).contains(&self.day) {
            "day"
        } else if !(0..=23).contains(&self.hour) {
            "hour"
        } else if !(0..=59).contains(&self.minute) {
            "minute"
        } else if !(0.0..60.0).contains(&self.second) {
            "second"
        } else {
            return Ok(());
        };
        Err(SkyError::Calendar { field })
    }
}

impl core::fmt::Display for Civil {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:06.3}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

/// A moment, carried on both timescales the sky needs at once.
///
/// Two clocks, and conflating them is the usual way a historical sky comes out
/// several minutes wrong. **Terrestrial Time** is uniform and is the argument
/// every series in this crate takes. **Universal Time** tracks the Earth's
/// actual rotation and is the argument sidereal time and therefore every
/// horizon takes. The difference between them is `delta-T`, which is 16.7
/// seconds in 1789 and 69 seconds today, and an implementation that used one
/// where the other belongs puts the whole sky wrong by that much of a rotation.
///
/// ```
/// use corvid_sky::{Civil, Instant};
///
/// let instant = Instant::from_civil(Civil::new(1789, 4, 28, 21, 0, 0.0))?;
///
/// // Sixteen and a bit seconds, recovered from the eclipse and occultation
/// // record rather than from a clock, because nobody was keeping one.
/// assert!((instant.delta_t() - 16.7).abs() < 0.2);
/// assert!(instant.terrestrial() > instant.universal());
/// # Ok::<(), corvid_sky::SkyError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Instant {
    universal: f64,
    terrestrial: f64,
}

impl Instant {
    /// A moment from a civil date and clock time in Universal Time.
    ///
    /// # Errors
    ///
    /// [`SkyError::Calendar`] when a field is outside the range
    /// [`Civil`] documents for it, and [`SkyError::NotFinite`] when the
    /// seconds are not a finite number.
    pub fn from_civil(civil: Civil) -> Result<Self, SkyError> {
        civil.check()?;
        let day = julian_day(&civil);
        if !day.is_finite() {
            return Err(SkyError::NotFinite);
        }
        Ok(Self {
            universal: day,
            terrestrial: day + deltat::seconds(civil.decimal_year()) / SECONDS_PER_DAY,
        })
    }

    /// A moment from a Julian day number on the Universal Time scale.
    ///
    /// # Errors
    ///
    /// [`SkyError::NotFinite`] when the argument is not a finite number.
    pub fn from_universal(julian_day: f64) -> Result<Self, SkyError> {
        if !julian_day.is_finite() {
            return Err(SkyError::NotFinite);
        }
        let delta = deltat::seconds(decimal_year_of(julian_day));
        Ok(Self {
            universal: julian_day,
            terrestrial: julian_day + delta / SECONDS_PER_DAY,
        })
    }

    /// A moment from a Julian day number on the Terrestrial Time scale.
    ///
    /// The delta-T lookup wants a calendar year, which is read off the
    /// argument directly: being a minute out in the date cannot change the
    /// year by enough to change the answer, since delta-T is a polynomial in
    /// centuries.
    ///
    /// # Errors
    ///
    /// [`SkyError::NotFinite`] when the argument is not a finite number.
    pub fn from_terrestrial(julian_day: f64) -> Result<Self, SkyError> {
        if !julian_day.is_finite() {
            return Err(SkyError::NotFinite);
        }
        let delta = deltat::seconds(decimal_year_of(julian_day));
        Ok(Self {
            universal: julian_day - delta / SECONDS_PER_DAY,
            terrestrial: julian_day,
        })
    }

    /// The Julian day number on the Universal Time scale.
    #[must_use]
    pub const fn universal(&self) -> f64 {
        self.universal
    }

    /// The Julian day number on the Terrestrial Time scale.
    #[must_use]
    pub const fn terrestrial(&self) -> f64 {
        self.terrestrial
    }

    /// `TT - UT1`, in seconds.
    #[must_use]
    pub fn delta_t(&self) -> f64 {
        (self.terrestrial - self.universal) * SECONDS_PER_DAY
    }

    /// Julian centuries of Terrestrial Time since J2000.0, which is the
    /// argument every series in this crate is written in.
    #[must_use]
    pub fn centuries(&self) -> f64 {
        (self.terrestrial - J2000) / DAYS_PER_CENTURY
    }

    /// This moment moved by a number of days of Universal Time.
    ///
    /// Delta-T is re-evaluated, so stepping a year forward a day at a time
    /// lands where stepping it in one go does.
    #[must_use]
    pub fn shift_days(&self, days: f64) -> Self {
        Self::from_universal(self.universal + days).unwrap_or(*self)
    }

    /// This moment moved by a number of seconds of Universal Time.
    #[must_use]
    pub fn shift_seconds(&self, seconds: f64) -> Self {
        self.shift_days(seconds / SECONDS_PER_DAY)
    }

    /// Midnight at the start of the Universal Time day this moment falls in.
    #[must_use]
    pub fn midnight(&self) -> Self {
        Self::from_universal(libm::floor(self.universal - 0.5) + 0.5).unwrap_or(*self)
    }

    /// The civil date and Universal clock time this moment is.
    #[must_use]
    pub fn civil(&self) -> Civil {
        civil_of(self.universal)
    }
}

/// The Julian day number of a civil date, Meeus, *Astronomical Algorithms*,
/// 2nd ed., equation 7.1.
fn julian_day(civil: &Civil) -> f64 {
    let (year, month) = if civil.month <= 2 {
        (civil.year - 1, civil.month + 12)
    } else {
        (civil.year, civil.month)
    };
    // The ten days Gregory took out. A date before the cut is a Julian date and
    // gets no century correction, which is what makes a fourteenth-century
    // instant land where its chronicle put it.
    let correction = if (civil.year, civil.month, civil.day) >= (1582, 10, 15) {
        let century = year.div_euclid(100);
        f64::from(2 - century + century.div_euclid(4))
    } else {
        0.0
    };
    libm::floor(365.25 * f64::from(year + 4716))
        + libm::floor(30.600_1 * f64::from(month + 1))
        + f64::from(civil.day)
        + civil.day_fraction()
        + correction
        - 1_524.5
}

/// The civil date of a Julian day number, Meeus, *Astronomical Algorithms*,
/// 2nd ed., chapter 7.
///
/// Every intermediate here is a whole number well inside an `f64`'s mantissa,
/// so the arithmetic stays in `f64` and narrows once, at the end.
fn civil_of(julian_day: f64) -> Civil {
    let shifted = julian_day + 0.5;
    let integral = libm::floor(shifted);
    let fraction = shifted - integral;
    let adjusted = if integral < 2_299_161.0 {
        integral
    } else {
        let centuries = libm::floor((integral - 1_867_216.25) / 36_524.25);
        integral + 1.0 + centuries - libm::floor(centuries / 4.0)
    } + 1_524.0;
    let cycle = libm::floor((adjusted - 122.1) / 365.25);
    let days = libm::floor(365.25 * cycle);
    let months = libm::floor((adjusted - days) / 30.600_1);
    let day = adjusted - days - libm::floor(30.600_1 * months);
    let month = if months < 14.0 {
        months - 1.0
    } else {
        months - 13.0
    };
    let year = if month > 2.0 {
        cycle - 4_716.0
    } else {
        cycle - 4_715.0
    };

    // The clock, rebuilt from the fraction of a day. Rounding to the
    // millisecond first is what stops a whole number of hours arriving as
    // `10:59:59.999_999_9`.
    let seconds = libm::round(fraction * SECONDS_PER_DAY * 1_000.0) / 1_000.0;
    let hour = libm::floor(seconds / 3_600.0);
    let minute = libm::floor((seconds - hour * 3_600.0) / 60.0);
    Civil {
        year: narrow(year),
        month: narrow(month),
        day: narrow(day),
        hour: narrow(hour),
        minute: narrow(minute),
        second: seconds - hour * 3_600.0 - minute * 60.0,
    }
}

/// The decimal year a Julian day number falls in, for the delta-T lookup.
fn decimal_year_of(julian_day: f64) -> f64 {
    let civil = civil_of(julian_day);
    civil.decimal_year()
}

/// A whole `f64` as an `i32`, saturating rather than wrapping.
fn narrow(value: f64) -> i32 {
    let wide = whole(value);
    i32::try_from(wide).unwrap_or(if wide < 0 { i32::MIN } else { i32::MAX })
}
