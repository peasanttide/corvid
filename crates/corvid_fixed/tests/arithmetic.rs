//! Arithmetic: saturation, wrapping, overflow detection, rounding, and the
//! total behavior of the degenerate cases.
//!
//! The 8-bit types have 65536 operand pairs, so multiplication and division are
//! checked exhaustively against a reference computed in `f64`. That covers every
//! sign combination, every boundary, and every rounding tie at once.

#![allow(
    clippy::float_cmp,
    reason = "comparisons are against exactly representable references"
)]

mod common;

use common::{I32_EDGES, Rng};
use corvid_fixed::{
    Angle8, Angle16, Angle32, Factor8, Factor16, Factor32, I0F8, I8F8, I24F8, Signed8, Signed16,
    Signed32,
};

/// The reference result of a fixed-point operation: the true value, rounded to
/// the type's resolution the way the implementation promises to round.
fn round_half_away(value: f64) -> f64 {
    if value >= 0.0 { (value + 0.5).floor() } else { (value - 0.5).ceil() }
}

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
                let expected = if a > 0 {
                    I0F8::MAX
                } else if a < 0 {
                    I0F8::MIN
                } else {
                    I0F8::ZERO
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
            assert_eq!(left.checked_div(right).is_none(), exact != expected, "{a} / {b}");
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
    assert_eq!(I8F8::ZERO.overflowing_add(I8F8::DELTA), (I8F8::DELTA, false));

    assert_eq!(Factor8::MAX.checked_add(Factor8::DELTA), None);
    assert_eq!(Factor8::ZERO.checked_sub(Factor8::DELTA), None);
    assert_eq!(Signed16::MAX.checked_add(Signed16::DELTA), None);
    assert_eq!(Signed16::MIN.checked_sub(Signed16::DELTA), None);
    assert_eq!(Signed16::MAX.checked_sub(Signed16::MAX), Some(Signed16::ZERO));
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
    assert_eq!(big.overflowing_mul(big).1, true);
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
fn multiplication_by_one_is_the_identity() {
    for bits in i16::MIN..=i16::MAX {
        let value = I8F8::from_bits(bits);
        assert_eq!(value.saturating_mul(I8F8::ONE), value, "I8F8 at {bits}");
    }
    for bits in 0..=u16::MAX {
        let value = Factor16::from_bits(bits);
        assert_eq!(value.mul(Factor16::ONE), value, "Factor16 at {bits}");
    }
    for bits in i16::MIN..=i16::MAX {
        let value = Signed16::from_bits(bits).canonicalize();
        assert_eq!(value.mul(Signed16::MAX), value, "Signed16 at {bits}");
        assert_eq!(value.mul(Signed16::MIN), -value, "Signed16 negated at {bits}");
    }
}

#[test]
fn division_by_one_is_the_identity() {
    for bits in i16::MIN..=i16::MAX {
        let value = I8F8::from_bits(bits);
        assert_eq!(value.saturating_div(I8F8::ONE), value, "I8F8 at {bits}");
    }
    for bits in 0..=u16::MAX {
        let value = Factor16::from_bits(bits);
        assert_eq!(value.saturating_div(Factor16::ONE), value, "Factor16 at {bits}");
    }
}

#[test]
fn the_factor_complement_is_exact() {
    for bits in 0..=u16::MAX {
        let value = Factor16::from_bits(bits);
        assert_eq!(value.complement().complement(), value);
        assert_eq!(
            value.complement().saturating_add(value),
            Factor16::ONE,
            "complement at {bits}"
        );
    }
    assert_eq!(Factor8::ZERO.complement(), Factor8::ONE);
    assert_eq!(Factor8::ONE.complement(), Factor8::ZERO);
}

#[test]
fn angles_wrap_under_arithmetic() {
    assert_eq!(Angle8::MAX + Angle8::DELTA, Angle8::ZERO);
    assert_eq!(Angle8::ZERO - Angle8::DELTA, Angle8::MAX);
    assert_eq!(Angle16::MAX + Angle16::DELTA, Angle16::ZERO);
    assert_eq!(Angle32::MAX + Angle32::DELTA, Angle32::ZERO);
    assert_eq!(-Angle16::ZERO, Angle16::ZERO);
    assert_eq!(-Angle16::QUARTER_TURN, Angle16::THREE_QUARTER_TURN);
    assert_eq!(Angle16::HALF_TURN + Angle16::HALF_TURN, Angle16::ZERO);

    // Turning by the same amount 2^16 times returns exactly where it started.
    let mut heading = Angle16::from_degrees(37.0);
    let step = Angle16::from_bits(1);
    for _ in 0..u32::from(u16::MAX) + 1 {
        heading += step;
    }
    assert_eq!(heading, Angle16::from_degrees(37.0));
}

#[test]
fn the_shortest_arc_is_never_more_than_half_a_turn() {
    let mut rng = Rng::new(0xa11c_e5);
    for _ in 0..20_000 {
        let a = Angle16::from_bits(rng.next_u32() as u16);
        let b = Angle16::from_bits(rng.next_u32() as u16);
        let arc = a.abs_diff(b);
        assert!(arc <= Angle16::HALF_TURN, "{a:?} to {b:?} gave {arc:?}");
        assert_eq!(arc, b.abs_diff(a), "abs_diff should be symmetric");
        // Stepping the arc from one end lands on the other, one way or another.
        assert!(a + arc == b || a - arc == b, "{a:?} +/- {arc:?} != {b:?}");
    }
    assert_eq!(Angle16::ZERO.abs_diff(Angle16::MAX), Angle16::DELTA);
    assert_eq!(Angle16::ZERO.abs_diff(Angle16::HALF_TURN), Angle16::HALF_TURN);
}

#[test]
fn interpolation_hits_both_endpoints_exactly() {
    let mut rng = Rng::new(0xbeef_cafe);
    for _ in 0..5_000 {
        let a = I24F8::from_bits(rng.next_u32() as i32);
        let b = I24F8::from_bits(rng.next_u32() as i32);
        assert_eq!(a.lerp(b, Factor32::ZERO), a);
        assert_eq!(a.lerp(b, Factor32::ONE), b);

        let mid = a.lerp(b, Factor32::from_f64(0.5));
        assert!(mid >= a.min(b) && mid <= a.max(b), "midpoint left the interval");
    }

    let f = Factor16::from_bits(1000);
    let g = Factor16::from_bits(60_000);
    assert_eq!(f.lerp(g, Factor16::ZERO), f);
    assert_eq!(f.lerp(g, Factor16::ONE), g);

    let s = Signed16::from_f64(-0.5);
    let t = Signed16::from_f64(0.5);
    assert_eq!(s.lerp(t, Factor16::ZERO), s);
    assert_eq!(s.lerp(t, Factor16::ONE), t);
    assert_eq!(s.lerp(t, Factor16::from_f64(0.5)), Signed16::ZERO);
}

#[test]
fn angle_interpolation_takes_the_short_way() {
    let a = Angle16::from_degrees(350.0);
    let b = Angle16::from_degrees(10.0);

    assert_eq!(a.lerp(b, Factor16::ZERO), a);
    assert_eq!(a.lerp(b, Factor16::ONE), b);

    // Halfway from 350 degrees to 10 degrees is 0, not 180.
    let midpoint = a.lerp(b, Factor16::from_f64(0.5));
    assert!(
        midpoint.abs_diff(Angle16::ZERO).to_degrees() < 1.0,
        "went the long way: {midpoint:?}"
    );

    // A quarter of the way across a 20 degree arc is 5 degrees along.
    let quarter = a.lerp(b, Factor16::from_f64(0.25));
    assert!((quarter.to_degrees() - 355.0).abs() < 1.0, "{quarter:?}");
}

#[test]
fn ordering_matches_the_numeric_order() {
    for bits in i16::MIN..i16::MAX {
        let low = I8F8::from_bits(bits);
        let high = I8F8::from_bits(bits + 1);
        assert!(low < high, "I8F8 order broke at {bits}");
        assert_eq!(low.min(high), low);
        assert_eq!(low.max(high), high);
    }

    // The snorm denormal compares equal to the canonical -1.0, and both sit
    // below every other value.
    assert_eq!(Signed8::from_bits(-128), Signed8::from_bits(-127));
    assert!(Signed8::from_bits(-128) < Signed8::from_bits(-126));
    assert!(Signed8::from_bits(-127) < Signed8::from_bits(-126));
    assert!(!(Signed8::from_bits(-128) < Signed8::from_bits(-127)));
}

#[test]
fn clamp_confines_without_panicking() {
    let low = I8F8::from_f64(-1.0);
    let high = I8F8::from_f64(1.0);
    assert_eq!(I8F8::from_f64(5.0).clamp(low, high), high);
    assert_eq!(I8F8::from_f64(-5.0).clamp(low, high), low);
    assert_eq!(I8F8::ZERO.clamp(low, high), I8F8::ZERO);
    // Reversed bounds would panic in the standard library; here the last bound
    // applied wins.
    assert_eq!(I8F8::ZERO.clamp(high, low), low);
}

#[test]
fn square_roots_are_correct_and_total() {
    for bits in 0..=i16::MAX {
        let value = I8F8::from_bits(bits);
        let expected = round_half_away((f64::from(bits) * 256.0).sqrt());
        let clamped = expected.min(f64::from(i16::MAX));
        assert_eq!(f64::from(value.sqrt().to_bits()), clamped, "sqrt of {bits}");
    }
    for bits in i16::MIN..0 {
        assert_eq!(I8F8::from_bits(bits).sqrt(), I8F8::ZERO);
        assert_eq!(I8F8::from_bits(bits).checked_sqrt(), None);
    }

    // Perfect squares come back exactly.
    for root in 0..=11_i32 {
        let square = I8F8::from_f64(f64::from(root * root));
        assert_eq!(square.sqrt().to_f64(), f64::from(root), "sqrt of {}^2", root);
    }

    // Factors and signed values root within their own range.
    for bits in 0..=u16::MAX {
        let expected = round_half_away((f64::from(bits) * f64::from(u16::MAX)).sqrt());
        assert_eq!(
            f64::from(Factor16::from_bits(bits).sqrt().to_bits()),
            expected,
            "Factor16 sqrt of {bits}"
        );
    }
    assert_eq!(Factor32::ONE.sqrt(), Factor32::ONE);
    assert_eq!(Factor32::ZERO.sqrt(), Factor32::ZERO);
    assert_eq!(Signed16::MAX.sqrt(), Signed16::MAX);
    assert_eq!(Signed16::MIN.sqrt(), Signed16::ZERO);
    assert_eq!(Signed16::MIN.checked_sqrt(), None);
}

#[test]
fn i0f8_square_roots_saturate() {
    // The square root of anything above 0.25 leaves I0F8's range.
    assert_eq!(I0F8::from_f64(0.25).sqrt(), I0F8::MAX);
    assert_eq!(I0F8::from_f64(0.0625).sqrt().to_f64(), 0.25);
    assert_eq!(I0F8::ZERO.sqrt(), I0F8::ZERO);
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
        assert_eq!(snorm.saturating_add(snorm).to_f64().abs() <= 1.0, true);

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

#[test]
fn arithmetic_is_available_in_const_context() {
    const A: I24F8 = I24F8::from_f64(1.5);
    const B: I24F8 = I24F8::from_f64(-0.25);
    const SUM: I24F8 = A.saturating_add(B);
    const PRODUCT: I24F8 = A.saturating_mul(B);
    const QUOTIENT: I24F8 = A.saturating_div(B);
    const ROOT: I24F8 = I24F8::from_f64(2.25).sqrt();
    const MIDPOINT: I24F8 = A.lerp(B, Factor32::from_f64(0.5));
    const CHECKED: Option<I24F8> = I24F8::MAX.checked_add(I24F8::DELTA);
    const OVERFLOWED: bool = I24F8::MAX.overflowing_add(I24F8::DELTA).1;

    assert_eq!(SUM.to_f64(), 1.25);
    assert_eq!(PRODUCT.to_f64(), -0.375);
    assert_eq!(QUOTIENT.to_f64(), -6.0);
    assert_eq!(ROOT.to_f64(), 1.5);
    assert_eq!(MIDPOINT.to_f64(), 0.625);
    assert_eq!(CHECKED, None);
    assert_eq!(OVERFLOWED, true);

    const FACTOR: Factor16 = Factor16::MAX.mul(Factor16::from_f64(0.5));
    const SNORM: Signed16 = Signed16::MIN.neg();
    const ARC: Angle16 = Angle16::HALF_TURN.wrapping_add(Angle16::QUARTER_TURN);
    assert_eq!(FACTOR, Factor16::from_f64(0.5));
    assert_eq!(SNORM, Signed16::MAX);
    assert_eq!(ARC, Angle16::THREE_QUARTER_TURN);
}
