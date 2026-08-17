//! `TT - UT1`, from the Espenak and Meeus polynomials.
//!
//! The Earth's rotation is not uniform and is not predictable, so the offset
//! between the timescale a series takes as its argument and the timescale a
//! horizon takes as its is measured rather than derived. Before the telescope
//! it is recovered from eclipse records, which is why the pre-1600 branches
//! below are fits to observations rather than a model of anything.
//!
//! The polynomials are those in Espenak and Meeus, *Five Millennium Canon of
//! Solar Eclipses: -1999 to +3000* (NASA/TP-2006-214141), Chapter 4, as
//! published by NASA's eclipse site. They assume a lunar secular acceleration
//! of `-25.858 arcsec/cy^2`; the correction to Stephenson and Morrison's
//! `-26 arcsec/cy^2` is `-0.000_012_932 (y - 1955)^2` seconds, which is 0.36
//! seconds at 1789 and is not applied here.

use crate::math::poly;

/// `TT - UT1` in seconds at a decimal year, from the Espenak-Meeus fit.
///
/// The argument is a *decimal* year -- `year + (month - 0.5) / 12` -- because
/// that is the argument the published polynomials take. Outside `-1999 ..=
/// 3000` the parabola on either end is an extrapolation and this crate makes no
/// claim about it.
pub(crate) fn seconds(decimal_year: f64) -> f64 {
    // The seventeenth and eighteenth centuries are the interesting ones for a
    // historical game, so they are checked first: the early modern telescopic
    // record is where delta-T stops being tens of minutes and becomes tens of
    // seconds.
    if (1700.0..1800.0).contains(&decimal_year) {
        let t = decimal_year - 1_700.0;
        return poly(&[8.83, 0.160_3, -0.005_928_5, 0.000_133_36], t) - t * t * t * t / 1_174_000.0;
    }
    if (1600.0..1700.0).contains(&decimal_year) {
        let t = decimal_year - 1_600.0;
        return poly(&[120.0, -0.980_8, -0.015_32], t) + t * t * t / 7_129.0;
    }
    if decimal_year >= 1_800.0 {
        return modern(decimal_year);
    }
    ancient(decimal_year)
}

/// 1800 onwards, plus the parabola that runs off the end of the record.
fn modern(decimal_year: f64) -> f64 {
    if decimal_year < 1_860.0 {
        return poly(
            &[
                13.72,
                -0.332_447,
                0.006_861_2,
                0.004_111_6,
                -0.000_374_36,
                0.000_012_127_2,
                -0.000_000_169_9,
                0.000_000_000_875,
            ],
            decimal_year - 1_800.0,
        );
    }
    if decimal_year < 1_900.0 {
        let t = decimal_year - 1_860.0;
        return poly(
            &[7.62, 0.573_7, -0.251_754, 0.016_806_68, -0.000_447_362_4],
            t,
        ) + t * t * t * t * t / 233_174.0;
    }
    if decimal_year < 1_920.0 {
        let t = decimal_year - 1_900.0;
        return poly(
            &[-2.79, 1.494_119, -0.059_893_9, 0.006_196_6, -0.000_197],
            t,
        );
    }
    if decimal_year < 1_941.0 {
        return poly(
            &[21.20, 0.844_93, -0.076_100, 0.002_093_6],
            decimal_year - 1_920.0,
        );
    }
    if decimal_year < 1_961.0 {
        let t = decimal_year - 1_950.0;
        return 29.07 + 0.407 * t - t * t / 233.0 + t * t * t / 2_547.0;
    }
    if decimal_year < 1_986.0 {
        let t = decimal_year - 1_975.0;
        return 45.45 + 1.067 * t - t * t / 260.0 - t * t * t / 718.0;
    }
    if decimal_year < 2_005.0 {
        return poly(
            &[
                63.86,
                0.334_5,
                -0.060_374,
                0.001_727_5,
                0.000_651_814,
                0.000_023_735_99,
            ],
            decimal_year - 2_000.0,
        );
    }
    if decimal_year < 2_050.0 {
        return poly(&[62.92, 0.322_17, 0.005_589], decimal_year - 2_000.0);
    }
    // Past the record entirely. The parabola is the long-term tidal trend and
    // the linear term is the seam that makes it meet the 2050 value; both come
    // straight from the source, and neither is a prediction anyone should hang
    // a claim on.
    let parabola = parabola(decimal_year);
    if decimal_year < 2_150.0 {
        return parabola - 0.562_8 * (2_150.0 - decimal_year);
    }
    parabola
}

/// Before 1600, where delta-T comes out of eclipse records rather than clocks.
fn ancient(decimal_year: f64) -> f64 {
    if decimal_year >= 500.0 {
        return poly(
            &[
                1_574.2,
                -556.01,
                71.234_72,
                0.319_781,
                -0.850_346_3,
                -0.005_050_998,
                0.008_357_207_3,
            ],
            (decimal_year - 1_000.0) / 100.0,
        );
    }
    if decimal_year >= -500.0 {
        return poly(
            &[
                10_583.6,
                -1_014.41,
                33.783_11,
                -5.952_053,
                -0.179_845_2,
                0.022_174_192,
                0.009_031_652_1,
            ],
            decimal_year / 100.0,
        );
    }
    parabola(decimal_year)
}

/// The long-term tidal parabola both ends of the fit run out into.
fn parabola(decimal_year: f64) -> f64 {
    let centuries = (decimal_year - 1_820.0) / 100.0;
    32.0 * centuries * centuries - 20.0
}
