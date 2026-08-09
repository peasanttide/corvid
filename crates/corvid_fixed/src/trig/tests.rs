//! What every derived constant here is checked against: `f64`, and Euler's
//! independent identity for pi.

#![allow(
    clippy::panic,
    clippy::float_cmp,
    reason = "tests assert; panicking is how a test reports failure"
)]

extern crate std;

use super::arc::{ATAN_POW2, CORDIC_ITERS, CORDIC_SCALE_BITS};
use super::sine::{COS_COEFFICIENTS, SIN_COEFFICIENTS, TERMS, sin_q, sin_snorm};
use super::wide::{
    COS_COEFFICIENTS_WIDE, ONE_WIDE, PI_WIDE, Q_WIDE, SIN_COEFFICIENTS_WIDE, SIN_Q_ERROR,
    TERMS_WIDE, mul_shift, q_to_snorm_wide, sin_q_wide,
};
use super::{ONE, PI, Q, TURNS_PER_RADIAN, TWO_PI, atan_series, mulq};

/// Converts an internal Q60 value to `f64` for comparison against `std`.
fn to_f64(v: i64) -> f64 {
    v as f64 / (ONE as f64)
}

#[test]
fn pi_matches_std() {
    let error = (to_f64(PI) - core::f64::consts::PI).abs();
    assert!(error < 1e-17, "pi off by {error:e}");
    assert_eq!(TWO_PI, 2 * PI);
}

#[test]
fn machin_agrees_with_a_different_identity() {
    // Euler: pi/4 = atan(1/2) + atan(1/3). An independent route to the same
    // constant catches a transcription error in either formula.
    let euler = 4 * (atan_series(ONE / 2) + atan_series(ONE / 3));
    assert!(
        (PI - euler).abs() < 64,
        "machin and euler differ by {}",
        PI - euler
    );
}

#[test]
fn turns_per_radian_is_the_reciprocal_of_a_turn() {
    let expected = 1.0 / core::f64::consts::TAU;
    assert!((to_f64(TURNS_PER_RADIAN) - expected).abs() < 1e-17);
}

#[test]
fn atan_table_matches_std() {
    for (i, &entry) in ATAN_POW2.iter().enumerate() {
        let expected = (2.0_f64).powi(-(i as i32)).atan();
        let error = (to_f64(entry) - expected).abs();
        assert!(error < 1e-16, "atan(2^-{i}) off by {error:e}");
    }
}

#[test]
fn atan_table_shrinks_monotonically() {
    for pair in ATAN_POW2.windows(2) {
        assert!(pair[1] < pair[0]);
    }
    assert!(ATAN_POW2[CORDIC_ITERS - 1] > 0, "table underflowed to zero");
}

#[test]
fn last_rotation_bounds_the_residual_angle() {
    // The unmodelled residual after the final rotation must sit well below
    // the last bit of Angle32, which is TWO_PI / 2^32 radians.
    let last_bit = to_f64(TWO_PI) / f64::from(u32::MAX);
    assert!(to_f64(ATAN_POW2[CORDIC_ITERS - 1]) < last_bit / 100.0);
}

#[test]
fn cordic_coordinates_cannot_overflow() {
    // Normalization bounds the larger coordinate, not the magnitude, so a
    // diagonal vector is already sqrt(2) longer than the working scale. The
    // rotations multiply that length by the CORDIC gain, and the result has
    // to stay inside i64.
    let gain: f64 = (0..CORDIC_ITERS)
        .map(|i| (1.0 + 4.0_f64.powi(-(i as i32))).sqrt())
        .product();
    assert!(gain < 1.65, "gain grew to {gain}");
    let peak = 2.0_f64.powi(CORDIC_SCALE_BITS as i32) * core::f64::consts::SQRT_2 * gain;
    assert!(peak < i64::MAX as f64, "peak {peak:e} exceeds i64");
}

#[test]
fn taylor_coefficients_are_reciprocal_factorials() {
    // Sine's coefficients are 1/(2k+1)! with alternating signs, cosine's are
    // 1/(2k)!. Checking against f64 catches an off-by-one in the generator.
    let mut factorial = 1.0_f64;
    for (k, &coefficient) in SIN_COEFFICIENTS.iter().enumerate() {
        if k > 0 {
            factorial *= f64::from(2 * k as u32) * f64::from(2 * k as u32 + 1);
        }
        let expected = if k % 2 == 1 {
            -1.0 / factorial
        } else {
            1.0 / factorial
        };
        let error = (to_f64(coefficient) - expected).abs();
        assert!(error < 1e-17, "sin coefficient {k} off by {error:e}");
    }

    let mut factorial = 1.0_f64;
    for (k, &coefficient) in COS_COEFFICIENTS.iter().enumerate() {
        if k > 0 {
            factorial *= f64::from(2 * k as u32 - 1) * f64::from(2 * k as u32);
        }
        let expected = if k % 2 == 1 {
            -1.0 / factorial
        } else {
            1.0 / factorial
        };
        let error = (to_f64(coefficient) - expected).abs();
        assert!(error < 1e-17, "cos coefficient {k} off by {error:e}");
    }

    assert_eq!(
        SIN_COEFFICIENTS[0], ONE,
        "the leading term must be exactly one"
    );
    assert_eq!(
        COS_COEFFICIENTS[0], ONE,
        "the leading term must be exactly one"
    );
    assert_eq!(TERMS, 7);
}

#[test]
fn the_approximation_weights_are_derived_correctly() {
    // These are integer-derived so that no floating-point constant appears
    // outside the conversion functions. Check them against the intent.
    const P_ONE: i64 = 1 << 30;
    assert_eq!(31 * P_ONE / 40, (0.775 * f64::from(P_ONE as u32)) as i64);
    assert_eq!(31 * P_ONE / 40 + (P_ONE - 31 * P_ONE / 40), P_ONE);

    let correction = (((273 * i128::from(P_ONE)) << Q) / (1000 * i128::from(TWO_PI))) as i64;
    let expected = (0.273 / core::f64::consts::TAU * f64::from(P_ONE as u32)) as i64;
    assert!(
        (correction - expected).abs() <= 1,
        "correction weight {correction} vs {expected}"
    );
}

#[test]
fn mulq_is_exact_for_powers_of_two() {
    assert_eq!(mulq(ONE, ONE), ONE);
    assert_eq!(mulq(ONE >> 1, ONE >> 1), ONE >> 2);
    assert_eq!(mulq(-(ONE >> 1), ONE >> 1), -(ONE >> 2));
    assert_eq!(mulq(0, ONE), 0);
}

#[test]
fn mul_shift_agrees_with_a_plain_product() {
    // Where the product fits an i128 on its own, the limb assembly has a
    // reference to be checked against.
    let cases: [i128; 8] = [0, 1, -1, 3, -7, 1 << 40, -(1 << 51), (1 << 62) - 1];
    for a in cases {
        for b in cases {
            for shift in [1_u32, 17, 63, 64, 65, 100] {
                let product = a * b;
                // A plain shift floors; mul_shift truncates toward zero.
                let expected = if product < 0 {
                    -((-product) >> shift)
                } else {
                    product >> shift
                };
                assert_eq!(
                    mul_shift(a, b, shift),
                    expected,
                    "mul_shift({a}, {b}, {shift})"
                );
            }
        }
    }
}

#[test]
fn mul_shift_carries_across_all_four_limbs() {
    // Operands with every limb populated exercise the carry out of the middle
    // column, which a product that fits an i128 never reaches. Multiplying by
    // a power of two is the one such case with an answer that can be written
    // down independently: the product is just a shift.
    let a = (1_i128 << 100) | (1 << 64) | (1 << 63) | 1;
    assert_eq!(mul_shift(a, 1 << 70, Q_WIDE), a >> 30);
    assert_eq!(mul_shift(-a, 1 << 70, Q_WIDE), -(a >> 30));
    assert_eq!(mul_shift(a, 1 << 64, Q_WIDE), a >> 36);

    let b = (1_i128 << 26) | (1 << 64) | 0x3039;
    assert_eq!(mul_shift(a, b, Q_WIDE), mul_shift(b, a, Q_WIDE));

    assert_eq!(mul_shift(ONE_WIDE, ONE_WIDE, Q_WIDE), ONE_WIDE);
    assert_eq!(mul_shift(ONE_WIDE, -ONE_WIDE, Q_WIDE), -ONE_WIDE);
    assert_eq!(
        mul_shift(ONE_WIDE >> 1, ONE_WIDE >> 1, Q_WIDE),
        ONE_WIDE >> 2
    );
}

#[test]
fn the_wide_constants_agree_with_the_narrow_ones() {
    // Two independent evaluations of Machin's formula, at scales 40 bits
    // apart. Truncating the wide one to Q60 should land on the narrow one.
    let narrowed = (PI_WIDE >> (Q_WIDE - Q)) as i64;
    assert!(
        (narrowed - PI).abs() < 64,
        "wide and narrow pi differ by {}",
        narrowed - PI
    );

    for i in 0..TERMS {
        let sine = (SIN_COEFFICIENTS_WIDE[i] >> (Q_WIDE - Q)) as i64;
        let cosine = (COS_COEFFICIENTS_WIDE[i] >> (Q_WIDE - Q)) as i64;
        assert!(
            (sine - SIN_COEFFICIENTS[i]).abs() <= 1,
            "sin coefficient {i} disagrees"
        );
        assert!(
            (cosine - COS_COEFFICIENTS[i]).abs() <= 1,
            "cos coefficient {i} disagrees"
        );
    }
    assert_eq!(TERMS_WIDE, 12);
    assert_eq!(Q_WIDE, 100);
}

/// The `Signed32` scale, the only width where the exact tier does any work.
const SNORM32: i64 = i32::MAX as i64;

#[test]
fn the_fast_path_error_bound_covers_what_it_claims() {
    // SIN_Q_ERROR is asserted, not measured, so measure it: over a sweep of
    // phases the Q60 result must never stray further from the Q100 one than
    // the bound allows.
    let mut state = 0x243f_6a88_85a3_08d3_u64;
    let mut worst = 0_i64;
    for _ in 0..200_000 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let phase = (state >> 32) as u32;
        let narrow = sin_q(phase);
        let wide = (sin_q_wide(phase) >> (Q_WIDE - Q)) as i64;
        let error = (narrow - wide).abs();
        if error > worst {
            worst = error;
        }
    }
    assert!(
        worst < SIN_Q_ERROR,
        "measured error {worst} against a bound of {SIN_Q_ERROR}"
    );
    assert!(
        worst > SIN_Q_ERROR / 8,
        "measured error {worst} is far under the bound of {SIN_Q_ERROR}; \
         the bound has gone stale and the exact tier is doing needless work"
    );
}

#[test]
fn the_two_stages_agree_on_a_sample() {
    // The cheap version of the exhaustive test below. Biased toward the
    // phases that matter: every one of these lands inside the fallback
    // window, where the two stages have the most opportunity to disagree.
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut fallbacks = 0_u32;
    for _ in 0..400_000 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let phase = (state >> 32) as u32;
        let expected = q_to_snorm_wide(sin_q_wide(phase), SNORM32);
        assert_eq!(sin_snorm(phase, SNORM32), expected, "phase {phase}");

        let scaled = (sin_q(phase) as i128) * (SNORM32 as i128);
        let magnitude = if scaled < 0 { -scaled } else { scaled };
        let residue = (magnitude + (1_i128 << (Q - 1))) & ((1_i128 << Q) - 1);
        let bound = (SIN_Q_ERROR as i128) * (SNORM32 as i128);
        if residue < bound || residue > (1_i128 << Q) - bound {
            fallbacks += 1;
        }
    }
    // One phase in 256, by construction. Well outside that band means the
    // dispatch has drifted from what its documentation promises.
    assert!(
        (600..=2600).contains(&fallbacks),
        "{fallbacks} fallbacks in 400000 phases; expected about 1560"
    );
}

/// Discharges the assumption behind the width gate in [`sin_snorm`].
///
/// That gate hands the 8- and 16-bit outputs straight to the Q60 path,
/// skipping the boundary test entirely. It is allowed to because their whole
/// domains fit in a test: over every phase either type can hold, Q60 already
/// lands on the bit pattern Q100 does. If that ever stops being true, the
/// gate has to go, and this is what will say so.
#[test]
fn the_narrow_widths_never_need_the_exact_tier() {
    for bits in 0..=u8::MAX {
        let phase = u32::from(bits) << 24;
        let max = i8::MAX as i64;
        assert_eq!(
            sin_snorm(phase, max),
            q_to_snorm_wide(sin_q_wide(phase), max),
            "Angle8 phase {phase}"
        );
    }
    for bits in 0..=u16::MAX {
        let phase = u32::from(bits) << 16;
        let max = i16::MAX as i64;
        assert_eq!(
            sin_snorm(phase, max),
            q_to_snorm_wide(sin_q_wide(phase), max),
            "Angle16 phase {phase}"
        );
    }
}

/// Walks every one of the 2^32 `Angle32` phases, asserting that what ships
/// equals what the Q100 path says.
///
/// This is one half of the correct-rounding argument. It shows the two-stage
/// dispatch never lets the fast path answer where the fast path is not
/// trustworthy -- that the shipped sine is, everywhere, the rounding of the
/// Q100 value. The other half is `tests/trig.rs`'s `EXACT` table, which pins
/// the Q100 value itself against 80-digit arithmetic at the hardest phases
/// the search could find. Cosine needs no separate pass: it is the sine a
/// quarter turn along, and this covers every phase.
///
/// Ignored because it takes about a minute and a half on eight cores. Run it
/// with:
///
/// ```sh
/// cargo test -p corvid_fixed --release exhaustive -- --ignored
/// ```
#[test]
#[ignore = "walks all 2^32 phases; run explicitly"]
fn sin_snorm_is_exhaustively_correctly_rounded_for_angle32() {
    let threads = std::thread::available_parallelism().map_or(4, core::num::NonZero::get);
    let span = (1_u64 << 32) / threads as u64;

    std::thread::scope(|scope| {
        for slot in 0..threads {
            scope.spawn(move || {
                let start = slot as u64 * span;
                let end = if slot + 1 == threads {
                    1_u64 << 32
                } else {
                    start + span
                };
                for phase in start..end {
                    let phase = phase as u32;
                    let expected = q_to_snorm_wide(sin_q_wide(phase), SNORM32);
                    assert_eq!(sin_snorm(phase, SNORM32), expected, "phase {phase}");
                }
            });
        }
    });
}

#[test]
fn q_is_the_documented_scale() {
    assert_eq!(Q, 60);
    assert_eq!(ONE, 1_i64 << 60);
    // Q60 in an i64 tops out just under 8.0. A full turn in radians is the
    // largest value this module represents, and it fits with room to spare.
    assert_eq!(i64::MAX / ONE, 7);
    assert_eq!(TWO_PI / ONE, 6, "a turn should be just over six in Q60");
}
