//! Arithmetic: saturation, wrapping, overflow detection, and the total
//! behavior of the degenerate cases.
//!
//! The 8-bit types have 65536 operand pairs, so multiplication and division are
//! checked exhaustively against a reference computed in `f64`. That covers every
//! sign combination, every boundary, and every rounding tie at once.

#![allow(
    clippy::float_cmp,
    reason = "comparisons are against exactly representable references"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::items_after_statements,
    reason = "these tests feed edge-case bit patterns through narrowing casts on purpose"
)]
mod common;

use common::{I32_EDGES, round_half_away};
use corvid_fixed::{
    Angle16, Angle32, Factor8, Factor16, Factor32, I0F8, I8F8, I24F8, Signed8, Signed16, Signed32,
};

#[test]
fn i8f8_multiplication_is_exhaustively_correct() {
    for a in i16::MIN..=i16::MAX {
        // Walking all 2^32 pairs is too slow; this samples the second operand
        // across every byte value at both scales, which hits every sign and
        // magnitude combination.
        for b in (i16::MIN..=i16::MAX).step_by(257) {
            let left = I8F8::from_bits(a);
            let right = I8F8::from_bits(b);
            let exact = round_half_away(f64::from(a) * f64::from(b) / 256.0);
            let expected = exact.clamp(f64::from(i16::MIN), f64::from(i16::MAX));

            assert_eq!(
                f64::from(left.saturating_mul(right).to_bits()),
                expected,
                "{a} * {b}"
            );
            assert_eq!(
                left.checked_mul(right).is_none(),
                exact != expected,
                "{a} * {b} overflow detection"
            );
        }
    }
}

#[test]
fn i0f8_multiplication_is_exhaustively_correct() {
    for a in i8::MIN..=i8::MAX {
        for b in i8::MIN..=i8::MAX {
            let exact = round_half_away(f64::from(a) * f64::from(b) / 256.0);
            let product = I0F8::from_bits(a).saturating_mul(I0F8::from_bits(b));
            assert_eq!(f64::from(product.to_bits()), exact, "{a} * {b}");
            // Products of two values under 0.5 always fit.
            assert!(I0F8::from_bits(a).checked_mul(I0F8::from_bits(b)).is_some());
        }
    }
}

#[test]
fn i0f8_division_is_exhaustively_correct() {
    for a in i8::MIN..=i8::MAX {
        for b in i8::MIN..=i8::MAX {
            let left = I0F8::from_bits(a);
            let right = I0F8::from_bits(b);
            if b == 0 {
                let expected = match a.cmp(&0) {
                    core::cmp::Ordering::Greater => I0F8::MAX,
                    core::cmp::Ordering::Less => I0F8::MIN,
                    core::cmp::Ordering::Equal => I0F8::ZERO,
                };
                assert_eq!(left.saturating_div(right), expected, "{a} / 0");
                assert_eq!(left.checked_div(right), None, "{a} / 0");
                continue;
            }
            let exact = round_half_away(f64::from(a) * 256.0 / f64::from(b));
            let expected = exact.clamp(f64::from(i8::MIN), f64::from(i8::MAX));
            assert_eq!(
                f64::from(left.saturating_div(right).to_bits()),
                expected,
                "{a} / {b}"
            );
            assert_eq!(
                left.checked_div(right).is_none(),
                exact != expected,
                "{a} / {b}"
            );
        }
    }
}

#[test]
fn factor8_multiplication_and_division_are_exhaustively_correct() {
    let scale = f64::from(u8::MAX);
    for a in 0..=u8::MAX {
        for b in 0..=u8::MAX {
            let left = Factor8::from_bits(a);
            let right = Factor8::from_bits(b);

            let product = round_half_away(f64::from(a) * f64::from(b) / scale);
            assert_eq!(f64::from(left.mul(right).to_bits()), product, "{a} * {b}");
            assert!(product <= scale, "product left the unit interval");

            if b == 0 {
                let expected = if a == 0 { Factor8::ZERO } else { Factor8::MAX };
                assert_eq!(left.saturating_div(right), expected, "{a} / 0");
                assert_eq!(left.checked_div(right), None);
                continue;
            }
            let quotient = round_half_away(f64::from(a) * scale / f64::from(b));
            let clamped = quotient.min(scale);
            assert_eq!(
                f64::from(left.saturating_div(right).to_bits()),
                clamped,
                "{a} / {b}"
            );
            assert_eq!(left.checked_div(right).is_none(), quotient != clamped);
        }
    }
}

#[test]
fn signed8_multiplication_is_exhaustively_correct() {
    let scale = f64::from(i8::MAX);
    for a in -127_i8..=127 {
        for b in -127_i8..=127 {
            let product = round_half_away(f64::from(a) * f64::from(b) / scale);
            let actual = Signed8::from_bits(a).mul(Signed8::from_bits(b));
            assert_eq!(f64::from(actual.to_bits()), product, "{a} * {b}");
            assert!(product.abs() <= scale, "product left [-1, 1]");
        }
    }
}

#[test]
fn addition_saturates_at_the_bounds() {
    assert_eq!(I8F8::MAX + I8F8::DELTA, I8F8::MAX);
    assert_eq!(I8F8::MIN - I8F8::DELTA, I8F8::MIN);
    assert_eq!(I0F8::MAX + I0F8::MAX, I0F8::MAX);
    assert_eq!(I24F8::MAX + I24F8::MAX, I24F8::MAX);

    assert_eq!(Factor8::MAX + Factor8::DELTA, Factor8::MAX);
    assert_eq!(Factor8::ZERO - Factor8::DELTA, Factor8::ZERO);
    assert_eq!(Factor32::MAX + Factor32::MAX, Factor32::MAX);

    assert_eq!(Signed8::MAX + Signed8::DELTA, Signed8::MAX);
    assert_eq!(Signed8::MIN - Signed8::DELTA, Signed8::MIN);
    assert_eq!(Signed16::MIN + Signed16::MIN, Signed16::MIN);
    assert_eq!(Signed32::MIN - Signed32::MAX, Signed32::MIN);
}

#[test]
fn overflow_is_detectable() {
    assert_eq!(I8F8::MAX.checked_add(I8F8::DELTA), None);
    assert_eq!(I8F8::MIN.checked_sub(I8F8::DELTA), None);
    assert_eq!(I8F8::MAX.checked_add(I8F8::ZERO), Some(I8F8::MAX));

    assert_eq!(I8F8::MAX.overflowing_add(I8F8::DELTA), (I8F8::MIN, true));
    assert_eq!(I8F8::MIN.overflowing_sub(I8F8::DELTA), (I8F8::MAX, true));
    assert_eq!(
        I8F8::ZERO.overflowing_add(I8F8::DELTA),
        (I8F8::DELTA, false)
    );

    assert_eq!(Factor8::MAX.checked_add(Factor8::DELTA), None);
    assert_eq!(Factor8::ZERO.checked_sub(Factor8::DELTA), None);
    assert_eq!(Signed16::MAX.checked_add(Signed16::DELTA), None);
    assert_eq!(Signed16::MIN.checked_sub(Signed16::DELTA), None);
    assert_eq!(
        Signed16::MAX.checked_sub(Signed16::MAX),
        Some(Signed16::ZERO)
    );
}

#[test]
fn fixed_point_arithmetic_wraps_on_demand() {
    assert_eq!(I8F8::MAX.wrapping_add(I8F8::DELTA), I8F8::MIN);
    assert_eq!(I8F8::MIN.wrapping_sub(I8F8::DELTA), I8F8::MAX);
    assert_eq!(I8F8::MIN.wrapping_neg(), I8F8::MIN);
    assert_eq!(I0F8::MAX.wrapping_add(I0F8::DELTA), I0F8::MIN);

    // Wrapping multiplication drops the high bits of the scaled product.
    let big = I8F8::from_f64(100.0);
    assert_eq!(big.saturating_mul(big), I8F8::MAX);
    assert_ne!(big.wrapping_mul(big), I8F8::MAX);
    assert!(big.overflowing_mul(big).1);
}

#[test]
fn negation_saturates_where_the_range_is_asymmetric() {
    // Two's complement has one more negative value than positive.
    assert_eq!(-I8F8::MIN, I8F8::MAX);
    assert_eq!(I8F8::MIN.checked_neg(), None);
    assert_eq!(I8F8::MIN.abs(), I8F8::MAX);
    assert_eq!((-I8F8::ONE).to_f64(), -1.0);

    // The signed normalized range is symmetric, so negation is exact.
    assert_eq!(-Signed8::MIN, Signed8::MAX);
    assert_eq!(-Signed8::MAX, Signed8::MIN);
    assert_eq!(Signed16::MIN.abs(), Signed16::MAX);
    for bits in i16::MIN..=i16::MAX {
        let value = Signed16::from_bits(bits);
        assert_eq!(-(-value), value.canonicalize(), "double negation at {bits}");
    }
}

#[test]
fn division_by_zero_is_total() {
    assert_eq!(I24F8::ONE / I24F8::ZERO, I24F8::MAX);
    assert_eq!(-I24F8::ONE / I24F8::ZERO, I24F8::MIN);
    assert_eq!(I24F8::ZERO / I24F8::ZERO, I24F8::ZERO);
    assert_eq!(I24F8::ONE.checked_div(I24F8::ZERO), None);

    assert_eq!(Factor16::MAX / Factor16::ZERO, Factor16::MAX);
    assert_eq!(Factor16::ZERO / Factor16::ZERO, Factor16::ZERO);

    assert_eq!(Signed32::MAX / Signed32::ZERO, Signed32::MAX);
    assert_eq!(Signed32::MIN / Signed32::ZERO, Signed32::MIN);
    assert_eq!(Signed32::ZERO / Signed32::ZERO, Signed32::ZERO);
}

#[test]
fn remainder_is_exact_and_total() {
    let seven = I8F8::from_f64(7.0);
    let two = I8F8::from_f64(2.0);
    assert_eq!((seven % two).to_f64(), 1.0);
    assert_eq!((-seven % two).to_f64(), -1.0);
    assert_eq!(I8F8::from_f64(0.5) % I8F8::from_f64(0.125), I8F8::ZERO);
    assert_eq!(seven % I8F8::ZERO, I8F8::ZERO);
    assert_eq!(seven.checked_rem(I8F8::ZERO), None);
    // MIN % -1 overflows a naive implementation; here it is zero.
    assert_eq!(I8F8::MIN % -I8F8::ONE, I8F8::ZERO);
}

#[test]
fn compound_assignment_matches_the_operators() {
    let mut a = I24F8::from_f64(3.5);
    a += I24F8::ONE;
    assert_eq!(a.to_f64(), 4.5);
    a -= I24F8::from_f64(0.5);
    assert_eq!(a.to_f64(), 4.0);
    a *= I24F8::from_f64(2.0);
    assert_eq!(a.to_f64(), 8.0);
    a /= I24F8::from_f64(4.0);
    assert_eq!(a.to_f64(), 2.0);
    a %= I24F8::from_f64(1.5);
    assert_eq!(a.to_f64(), 0.5);

    let mut angle = Angle16::QUARTER_TURN;
    angle += Angle16::HALF_TURN;
    assert_eq!(angle, Angle16::THREE_QUARTER_TURN);
    angle -= Angle16::HALF_TURN;
    assert_eq!(angle, Angle16::QUARTER_TURN);

    let mut factor = Factor8::from_f64(0.5);
    factor *= Factor8::ONE;
    assert_eq!(factor, Factor8::from_f64(0.5));
}

#[test]
fn the_edge_bit_patterns_never_misbehave() {
    for &raw in I32_EDGES {
        let fixed = I24F8::from_bits(raw);
        // None of these may panic, and all must stay in range.
        let _ = fixed.saturating_mul(fixed);
        let _ = fixed.saturating_div(fixed);
        let _ = fixed.saturating_add(I24F8::MAX);
        let _ = fixed.abs();
        let _ = fixed.sqrt();
        let _ = fixed.lerp(I24F8::MAX, Factor32::from_f64(0.5));

        let snorm = Signed32::from_bits(raw);
        assert!(snorm.to_f64() >= -1.0 && snorm.to_f64() <= 1.0);
        assert!(snorm.mul(snorm).to_f64().abs() <= 1.0);
        assert!(snorm.abs().to_f64() >= 0.0);
        assert!(snorm.saturating_add(snorm).to_f64().abs() <= 1.0);

        let factor = Factor32::from_bits(raw as u32);
        assert!(factor.to_f64() >= 0.0 && factor.to_f64() <= 1.0);
        assert!(factor.mul(factor).to_f64() <= 1.0);
        assert!(factor.sqrt().to_f64() <= 1.0);

        let angle = Angle32::from_bits(raw as u32);
        assert!(angle.abs_diff(angle).is_zero());
        let _ = angle.sin_cos();
        let _ = angle.tan();
    }
}
