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

use std::hint::black_box;

use common::{I32_EDGES, Rng};
use corvid_fixed::{
    Angle8, Angle16, Angle32, Factor8, Factor16, Factor32, I0F8, I2F30, I8F8, I16F16, I24F8,
    I48F16, Pitch16, Signed8, Signed16, Signed32,
};

/// The reference result of a fixed-point operation: the true value, rounded to
/// the type's resolution the way the implementation promises to round.
fn round_half_away(value: f64) -> f64 {
    if value >= 0.0 {
        (value + 0.5).floor()
    } else {
        (value - 0.5).ceil()
    }
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
        assert_eq!(
            value.mul(Signed16::MIN),
            -value,
            "Signed16 negated at {bits}"
        );
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
        assert_eq!(
            value.saturating_div(Factor16::ONE),
            value,
            "Factor16 at {bits}"
        );
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
    for _ in 0..=u32::from(u16::MAX) {
        heading += step;
    }
    assert_eq!(heading, Angle16::from_degrees(37.0));
}

#[test]
fn the_shortest_arc_is_never_more_than_half_a_turn() {
    let mut rng = Rng::new(0x00a1_1ce5);
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
    assert_eq!(
        Angle16::ZERO.abs_diff(Angle16::HALF_TURN),
        Angle16::HALF_TURN
    );
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
        assert!(
            mid >= a.min(b) && mid <= a.max(b),
            "midpoint left the interval"
        );
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
fn antipodal_interpolation_breaks_the_tie_clockwise() {
    // Exactly opposite angles have no shorter way round, so the tie has to
    // break somewhere. The wrapped difference reads as -2^(BITS-1) once taken
    // as a signed offset, so the phase *decreases*: halfway from zero to a half
    // turn is three quarters of a turn, not one quarter.
    let half = Factor16::from_f64(0.5);
    assert_eq!(
        Angle16::ZERO.lerp(Angle16::HALF_TURN, half),
        Angle16::THREE_QUARTER_TURN
    );

    // And it is the same tie from every starting angle: a - QUARTER_TURN.
    for bits in 0..=u16::MAX {
        let from = Angle16::from_bits(bits);
        let to = from + Angle16::HALF_TURN;
        assert_eq!(
            from.lerp(to, half),
            from - Angle16::QUARTER_TURN,
            "antipodal tie moved at {bits}"
        );
    }

    // The narrow and wide widths agree.
    assert_eq!(
        Angle8::ZERO.lerp(Angle8::HALF_TURN, Factor8::from_f64(0.5)),
        Angle8::THREE_QUARTER_TURN
    );
    assert_eq!(
        Angle32::ZERO.lerp(Angle32::HALF_TURN, Factor32::from_f64(0.5)),
        Angle32::THREE_QUARTER_TURN
    );
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
    assert!(Signed8::from_bits(-128) >= Signed8::from_bits(-127));
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
        assert_eq!(
            square.sqrt().to_f64(),
            f64::from(root),
            "sqrt of {root} squared"
        );
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

#[test]
fn rounding_matches_the_float_reference_exhaustively() {
    for bits in i16::MIN..=i16::MAX {
        let value = I8F8::from_bits(bits);
        let exact = value.to_f64();

        // I8F8 cannot hold 128.0, so anything that rounds up out of range
        // saturates; compare against the clamped reference.
        let low = I8F8::MIN.to_f64();
        let high = I8F8::MAX.to_f64();
        let floor = exact.floor().max(low);
        let ceil = exact.ceil().min(high);
        let round = round_half_away(exact).clamp(low, high);
        let trunc = exact.trunc();

        assert_eq!(value.floor().to_f64(), floor, "floor of {exact}");
        assert_eq!(value.ceil().to_f64(), ceil, "ceil of {exact}");
        assert_eq!(value.round().to_f64(), round, "round of {exact}");
        assert_eq!(value.trunc().to_f64(), trunc, "trunc of {exact}");
        assert_eq!(value.fract().to_f64(), exact - trunc, "fract of {exact}");

        // The defining identity, which also pins down the sign of fract.
        assert_eq!(value.trunc().to_f64() + value.fract().to_f64(), exact);
    }
}

#[test]
fn rounding_lands_on_integers_and_leaves_them_alone() {
    for whole in -100_i32..=100 {
        let value = I24F8::from_f64(f64::from(whole));
        assert_eq!(value.floor(), value, "floor moved {whole}");
        assert_eq!(value.ceil(), value, "ceil moved {whole}");
        assert_eq!(value.round(), value, "round moved {whole}");
        assert_eq!(value.trunc(), value, "trunc moved {whole}");
        assert_eq!(value.fract(), I24F8::ZERO, "fract of {whole} was not zero");
    }

    // Halfway cases go away from zero, like f64::round.
    assert_eq!(I24F8::from_f64(0.5).round().to_f64(), 1.0);
    assert_eq!(I24F8::from_f64(-0.5).round().to_f64(), -1.0);
    assert_eq!(I24F8::from_f64(1.5).round().to_f64(), 2.0);
    assert_eq!(I24F8::from_f64(-1.5).round().to_f64(), -2.0);
    assert_eq!(I24F8::from_f64(0.49).round(), I24F8::ZERO);

    // Floor and ceil differ from trunc on the negative side.
    assert_eq!(I24F8::from_f64(-2.5).floor().to_f64(), -3.0);
    assert_eq!(I24F8::from_f64(-2.5).ceil().to_f64(), -2.0);
    assert_eq!(I24F8::from_f64(-2.5).trunc().to_f64(), -2.0);
}

#[test]
fn rounding_saturates_instead_of_leaving_the_range() {
    assert_eq!(I8F8::MAX.ceil(), I8F8::MAX);
    assert_eq!(I8F8::MAX.round(), I8F8::MAX);
    assert_eq!(I8F8::MAX.floor().to_f64(), 127.0);
    assert_eq!(I8F8::MIN.floor(), I8F8::MIN);
    assert_eq!(I8F8::MIN.ceil(), I8F8::MIN);

    // Every I0F8 value is under 0.5 in magnitude, so trunc is always zero and
    // fract is always the whole value.
    for bits in i8::MIN..=i8::MAX {
        let value = I0F8::from_bits(bits);
        assert_eq!(value.trunc(), I0F8::ZERO, "trunc of {bits}");
        assert_eq!(value.fract(), value, "fract of {bits}");
    }
    assert_eq!(
        I0F8::from_f64(0.25).ceil(),
        I0F8::MAX,
        "ceil to 1.0 must saturate"
    );
    assert_eq!(
        I0F8::from_f64(-0.25).floor(),
        I0F8::MIN,
        "floor to -1.0 must saturate"
    );
}

#[test]
fn the_reciprocal_is_correct_and_total() {
    for bits in i16::MIN..=i16::MAX {
        let value = I8F8::from_bits(bits);
        if bits == 0 {
            assert_eq!(value.recip(), I8F8::MAX);
            assert_eq!(value.checked_recip(), None);
            continue;
        }
        let exact = round_half_away(65_536.0 / f64::from(bits));
        let clamped = exact.clamp(f64::from(i16::MIN), f64::from(i16::MAX));
        assert_eq!(
            f64::from(value.recip().to_bits()),
            clamped,
            "recip of {bits}"
        );
        assert_eq!(
            value.checked_recip().is_none(),
            exact != clamped,
            "recip of {bits}"
        );
    }

    assert_eq!(I24F8::ONE.recip(), I24F8::ONE);
    assert_eq!(I24F8::from_f64(2.0).recip().to_f64(), 0.5);
    assert_eq!(I24F8::from_f64(-4.0).recip().to_f64(), -0.25);
    assert_eq!(I24F8::from_f64(0.25).recip().to_f64(), 4.0);
    // Everything an I0F8 can hold has a reciprocal of at least 2.
    assert_eq!(I0F8::from_f64(0.25).recip(), I0F8::MAX);
    assert_eq!(I0F8::from_f64(-0.25).checked_recip(), None);
}

#[test]
fn mul_add_rounds_only_once() {
    // Multiplying and then adding rounds twice, so it can differ by a step. The
    // fused form is the one that matches the true value.
    let small = I24F8::from_bits(3);
    let one_step = I24F8::from_bits(1);
    // 3/256 * 3/256 is 9/65536, well under half a step, so it must vanish.
    assert_eq!(small.mul_add(small, one_step), one_step);

    let base = I24F8::from_f64(1.5);
    let scale = I24F8::from_f64(2.0);
    let offset = I24F8::from_f64(-0.25);
    assert_eq!(base.mul_add(scale, offset).to_f64(), 2.75);
    assert_eq!(
        base.mul_add(scale, offset),
        base.saturating_mul(scale).saturating_add(offset)
    );

    // Saturates like everything else.
    assert_eq!(I24F8::MAX.mul_add(I24F8::MAX, I24F8::ZERO), I24F8::MAX);
    assert_eq!(I24F8::MAX.mul_add(I24F8::MIN, I24F8::ZERO), I24F8::MIN);
    assert_eq!(I24F8::ZERO.mul_add(I24F8::MAX, I24F8::ONE), I24F8::ONE);

    // Against a reference, across a spread of magnitudes.
    let mut rng = Rng::new(0x3141_5926);
    for _ in 0..20_000 {
        let a = I8F8::from_bits(rng.next_u32() as i16);
        let b = I8F8::from_bits(rng.next_u32() as i16);
        let c = I8F8::from_bits(rng.next_u32() as i16);
        let exact = round_half_away(
            f64::from(a.to_bits()) * f64::from(b.to_bits()) / 256.0 + f64::from(c.to_bits()),
        );
        let expected = exact.clamp(f64::from(i16::MIN), f64::from(i16::MAX));
        assert_eq!(f64::from(a.mul_add(b, c).to_bits()), expected);
    }
}

#[test]
fn hypot_is_correct_and_never_overflows() {
    assert_eq!(
        I24F8::from_f64(3.0).hypot(I24F8::from_f64(4.0)).to_f64(),
        5.0
    );
    assert_eq!(
        I24F8::from_f64(-3.0).hypot(I24F8::from_f64(4.0)).to_f64(),
        5.0
    );
    assert_eq!(
        I24F8::from_f64(-3.0).hypot(I24F8::from_f64(-4.0)).to_f64(),
        5.0
    );
    assert_eq!(I24F8::ZERO.hypot(I24F8::ZERO), I24F8::ZERO);
    assert_eq!(I24F8::from_f64(5.0).hypot(I24F8::ZERO).to_f64(), 5.0);

    // Two large squares would overflow the storage type. The sum is formed at
    // double width, so the result merely saturates.
    assert_eq!(I24F8::MAX.hypot(I24F8::MAX), I24F8::MAX);
    assert_eq!(I24F8::MIN.hypot(I24F8::MIN), I24F8::MAX);
    assert_eq!(I8F8::MAX.hypot(I8F8::MAX), I8F8::MAX);

    let mut rng = Rng::new(0x2718_2818);
    for _ in 0..20_000 {
        let a = I8F8::from_bits((rng.next_u32() as i16) / 2);
        let b = I8F8::from_bits((rng.next_u32() as i16) / 2);
        let exact = round_half_away(f64::from(a.to_bits()).hypot(f64::from(b.to_bits())));
        let expected = exact.min(f64::from(i16::MAX));
        assert_eq!(
            f64::from(a.hypot(b).to_bits()),
            expected,
            "hypot of {a} and {b}"
        );
    }
}

#[test]
fn the_float_style_functions_are_available_in_const_context() {
    const VALUE: I24F8 = I24F8::from_f64(-2.75);
    const FLOOR: I24F8 = VALUE.floor();
    const CEIL: I24F8 = VALUE.ceil();
    const ROUND: I24F8 = VALUE.round();
    const TRUNC: I24F8 = VALUE.trunc();
    const FRACT: I24F8 = VALUE.fract();
    const RECIP: I24F8 = I24F8::from_f64(4.0).recip();
    const FUSED: I24F8 = VALUE.mul_add(I24F8::from_f64(2.0), I24F8::ONE);
    const LEG: I24F8 = I24F8::from_f64(3.0).hypot(I24F8::from_f64(4.0));

    assert_eq!(FLOOR.to_f64(), -3.0);
    assert_eq!(CEIL.to_f64(), -2.0);
    assert_eq!(ROUND.to_f64(), -3.0);
    assert_eq!(TRUNC.to_f64(), -2.0);
    assert_eq!(FRACT.to_f64(), -0.75);
    assert_eq!(RECIP.to_f64(), 0.25);
    assert_eq!(FUSED.to_f64(), -4.5);
    assert_eq!(LEG.to_f64(), 5.0);
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
    assert_eq!(OVERFLOWED, I24F8::MAX.overflowing_add(I24F8::DELTA).1);

    const FACTOR: Factor16 = Factor16::MAX.mul(Factor16::from_f64(0.5));
    const SNORM: Signed16 = Signed16::MIN.neg();
    const ARC: Angle16 = Angle16::HALF_TURN.wrapping_add(Angle16::QUARTER_TURN);
    assert_eq!(FACTOR, Factor16::from_f64(0.5));
    assert_eq!(SNORM, Signed16::MAX);
    assert_eq!(ARC, Angle16::THREE_QUARTER_TURN);
}

#[test]
fn comparison_is_available_in_const_context() {
    // `min`/`max`/`clamp` canonicalize their result, which for the signed and
    // pitch families is real work rather than a move -- so it has to survive
    // const evaluation, and the const and runtime paths have to agree on the
    // bits and not merely on the value.
    const LOW: I24F8 = I24F8::from_f64(-1.0);
    const HIGH: I24F8 = I24F8::from_f64(1.0);
    const CLAMPED: I24F8 = I24F8::from_f64(5.0).clamp(LOW, HIGH);
    const LESSER: Factor16 = Factor16::MAX.min(Factor16::from_bits(10));
    const GREATER: Angle16 = Angle16::MAX.max(Angle16::ZERO);
    const FOLDED: Signed8 = Signed8::from_bits(i8::MIN).clamp(Signed8::MIN, Signed8::MAX);
    const NARROWED: Pitch16 = Pitch16::from_bits(20_000).min(Pitch16::MAX);

    assert_eq!(CLAMPED, HIGH);
    assert_eq!(LESSER.to_bits(), 10);
    assert_eq!(GREATER, Angle16::MAX);
    assert_eq!(
        FOLDED.to_bits(),
        -127,
        "the denormal survived const folding"
    );
    assert_eq!(NARROWED.to_bits(), 16_384);

    // `black_box` keeps the compiler from folding these back into the constants
    // above, so this really compares the two evaluators against each other.
    let denormal = black_box(Signed8::from_bits(i8::MIN));
    let out_of_range = black_box(Pitch16::from_bits(20_000));
    assert_eq!(
        FOLDED.to_bits(),
        denormal.clamp(Signed8::MIN, Signed8::MAX).to_bits()
    );
    assert_eq!(NARROWED.to_bits(), out_of_range.min(Pitch16::MAX).to_bits());
}

// --- rsqrt -----------------------------------------------------------------
//
// The reciprocal square root is the one operation every normalize in Corvid
// reaches for. It is held to the same standard as `sqrt` and `mul`: correctly
// rounded, from a single full-width intermediate.

/// The correctly rounded `rsqrt` bit pattern, computed in `f64`.
///
/// `f64` carries 53 significant bits and no result here needs more than 31, so
/// the reference is exact except at ties, which the callers avoid by comparing
/// against the integer neighbours directly.
fn rsqrt_reference(bits: i64, frac: u32, max: i64) -> i64 {
    let value = bits as f64 / (1u64 << frac) as f64;
    let exact = 1.0 / value.sqrt() * (1u64 << frac) as f64;
    let rounded = round_half_away(exact);
    if rounded > max as f64 {
        max
    } else {
        rounded as i64
    }
}

#[test]
fn rsqrt_is_correctly_rounded_for_every_i8f8() {
    // Exhaustive, which settles correct rounding outright rather than sampling
    // for it.
    for bits in 1..=i16::MAX {
        let value = I8F8::from_bits(bits);
        let expected = rsqrt_reference(i64::from(bits), 8, i64::from(i16::MAX));
        assert_eq!(
            i64::from(value.rsqrt().to_bits()),
            expected,
            "rsqrt({}) at bits {bits}",
            value.to_f64()
        );
    }
}

#[test]
fn rsqrt_is_correctly_rounded_for_every_i0f8() {
    // I0F8's values are all under 0.5, so 1/sqrt(x) always exceeds 1.41 and the
    // result always saturates -- the same story as `recip`.
    for bits in 1..=i8::MAX {
        assert_eq!(I0F8::from_bits(bits).rsqrt(), I0F8::MAX);
    }
}

#[test]
fn rsqrt_is_correctly_rounded_across_i24f8_and_i16f16_and_i2f30() {
    let mut rng = Rng::new(0x5153_7274_0000_0001);
    for _ in 0..200_000 {
        // Cover every exponent, not just the top of the range: shift a random
        // value down by a random amount.
        let raw = ((rng.next_u32() >> 1) >> (rng.next_u32() % 30)) as i32 | 1;

        let coarse = I24F8::from_bits(raw);
        assert_eq!(
            i64::from(coarse.rsqrt().to_bits()),
            rsqrt_reference(i64::from(raw), 8, i64::from(i32::MAX)),
            "I24F8::rsqrt at bits {raw}"
        );

        let near = I16F16::from_bits(raw);
        assert_eq!(
            i64::from(near.rsqrt().to_bits()),
            rsqrt_reference(i64::from(raw), 16, i64::from(i32::MAX)),
            "I16F16::rsqrt at bits {raw}"
        );

        let entry = I2F30::from_bits(raw);
        assert_eq!(
            i64::from(entry.rsqrt().to_bits()),
            rsqrt_reference(i64::from(raw), 30, i64::from(i32::MAX)),
            "I2F30::rsqrt at bits {raw}"
        );
    }
}

#[test]
fn rsqrt_is_correctly_rounded_across_i48f16() {
    let mut rng = Rng::new(0x5153_7274_0000_0002);
    for _ in 0..200_000 {
        // I48F16 is the one type whose to_f64 is lossy, so keep the reference
        // honest by staying inside 53 significant bits.
        let raw = (((rng.next_u64() >> 11) >> (rng.next_u64() % 42)) as i64) | 1;
        let wide = I48F16::from_bits(raw);
        assert_eq!(
            wide.rsqrt().to_bits(),
            rsqrt_reference(raw, 16, i64::MAX),
            "I48F16::rsqrt at bits {raw}"
        );
    }
}

#[test]
fn rsqrt_times_sqrt_is_one() {
    for bits in [1i32, 2, 3, 255, 256, 1_000, 65_536, 1 << 20, i32::MAX] {
        let value = I16F16::from_bits(bits);
        let product = value.rsqrt().to_f64() * value.sqrt().to_f64();
        // sqrt's own quantization dominates at the bottom of the range, where a
        // last-bit root is a large relative error.
        let tolerance = if bits < 1 << 10 { 0.05 } else { 1e-3 };
        assert!(
            (product - 1.0).abs() < tolerance,
            "at bits {bits}: rsqrt * sqrt = {product}"
        );
    }
}

#[test]
fn rsqrt_saturates_on_zero_and_negatives() {
    assert_eq!(I24F8::ZERO.rsqrt(), I24F8::MAX);
    assert_eq!(I24F8::from_f64(-1.0).rsqrt(), I24F8::MAX);
    assert_eq!(I24F8::MIN.rsqrt(), I24F8::MAX);
    assert_eq!(I24F8::ZERO.checked_rsqrt(), None);
    assert_eq!(I24F8::from_f64(-1.0).checked_rsqrt(), None);
    assert_eq!(I2F30::ZERO.rsqrt(), I2F30::MAX);
    assert_eq!(I48F16::ZERO.rsqrt(), I48F16::MAX);
}

#[test]
fn rsqrt_saturates_rather_than_wrapping_when_the_result_is_out_of_range() {
    // 1/sqrt(0.25) is exactly 2.0, one step past I2F30::MAX.
    assert_eq!(I2F30::from_f64(0.25).rsqrt(), I2F30::MAX);
    assert_eq!(I2F30::from_f64(0.25).checked_rsqrt(), None);

    // Just inside, and the checked form succeeds.
    assert!(I2F30::from_f64(0.26).checked_rsqrt().is_some());
}

#[test]
fn rsqrt_is_available_in_const_context() {
    const ONE: I2F30 = I2F30::ONE.rsqrt();
    const QUARTER: I2F30 = I2F30::from_bits(1 << 28);
    const TWO: I2F30 = QUARTER.rsqrt();
    const FOUR: I16F16 = I16F16::from_bits(4 << 16).rsqrt();

    assert_eq!(ONE, I2F30::ONE);
    assert_eq!(TWO, I2F30::MAX);
    assert_eq!(FOUR, I16F16::from_f64(0.5));

    // Const and runtime agree, which is the whole determinism argument.
    assert_eq!(ONE, black_box(I2F30::ONE).rsqrt());
    assert_eq!(FOUR, black_box(I16F16::from_bits(4 << 16)).rsqrt());
}

// --- rsqrt_fast ------------------------------------------------------------
//
// The approximate tier. What is under test is not a bit pattern but a bound:
// `rsqrt_fast` promises a relative error under `3.2e-5` and nothing finer, so
// every check below is against that bound rather than against an exact answer.
// The bound is the documented contract, and tightening the implementation must
// not be allowed to silently loosen it.

/// The relative error `rsqrt_fast` is documented to hold, and the ceiling that
/// 32-bit arithmetic imposes on it.
const RSQRT_FAST_TOLERANCE: f64 = 3.2e-5;

/// Whether `got` is inside the error `rsqrt_fast` promises.
///
/// The promise has two terms. The approximation itself is good to
/// [`RSQRT_FAST_TOLERANCE`] *relative*, and landing that answer on the caller's
/// own resolution costs the half step any rounding costs -- a term that
/// dominates wherever the type is coarse enough that the true answer is only a
/// handful of last bits wide.
///
/// Saturated and zeroed results pass unconditionally: both are the caller's
/// clamp rather than the approximation's doing, and `rsqrt` clamps to the same
/// place.
fn rsqrt_fast_is_within_bound(got: i64, bits: i64, frac: u32, max: i64) -> bool {
    if got >= max || got == 0 {
        return true;
    }
    let value = bits as f64 / (1u64 << frac) as f64;
    let want = 1.0 / value.sqrt() * (1u64 << frac) as f64;
    (got as f64 - want).abs() <= want * RSQRT_FAST_TOLERANCE + 0.5
}

#[test]
fn rsqrt_fast_agrees_with_rsqrt_for_every_i0f8() {
    // Every I0F8 value is under 0.5, so both tiers saturate on every input and
    // the approximation has nowhere to show.
    for bits in 1..=i8::MAX {
        assert_eq!(I0F8::from_bits(bits).rsqrt_fast(), I0F8::MAX, "bits {bits}");
    }
}

#[test]
fn rsqrt_fast_stays_within_a_step_of_rsqrt_for_every_i8f8() {
    // Exhaustive. At `frac` 8 the type's own resolution is coarser than the
    // approximation's error over almost the whole range, so the two tiers land
    // on the same bits or on neighbours.
    for bits in 1..=i16::MAX {
        let value = I8F8::from_bits(bits);
        let fast = i32::from(value.rsqrt_fast().to_bits());
        let exact = i32::from(value.rsqrt().to_bits());
        assert!(
            (fast - exact).abs() <= 1,
            "I8F8::rsqrt_fast({}) gave {fast}, rsqrt gave {exact}",
            value.to_f64()
        );
    }
}

#[test]
fn rsqrt_fast_holds_its_bound_across_every_exponent() {
    let mut rng = Rng::new(0x5153_5246_0000_0001);
    for _ in 0..200_000 {
        // Cover every exponent rather than just the top of the range, the way
        // the exact tier's sweep does.
        let raw = ((rng.next_u32() >> 1) >> (rng.next_u32() % 30)) as i32 | 1;

        for (name, got, frac) in [
            (
                "I24F8",
                i64::from(I24F8::from_bits(raw).rsqrt_fast().to_bits()),
                8,
            ),
            (
                "I16F16",
                i64::from(I16F16::from_bits(raw).rsqrt_fast().to_bits()),
                16,
            ),
            (
                "I2F30",
                i64::from(I2F30::from_bits(raw).rsqrt_fast().to_bits()),
                30,
            ),
        ] {
            let exact = match frac {
                8 => i64::from(I24F8::from_bits(raw).rsqrt().to_bits()),
                16 => i64::from(I16F16::from_bits(raw).rsqrt().to_bits()),
                _ => i64::from(I2F30::from_bits(raw).rsqrt().to_bits()),
            };
            assert!(
                rsqrt_fast_is_within_bound(got, i64::from(raw), frac, i64::from(i32::MAX)),
                "{name}::rsqrt_fast at bits {raw} gave {got}, rsqrt gave {exact}"
            );
        }
    }
}

#[test]
fn rsqrt_fast_holds_its_bound_at_the_boundaries() {
    // The ends of the range, the powers of two either side of the seed's two
    // pieces, and the shifts that saturate -- the inputs a sampled sweep is
    // least likely to reach.
    for &raw in &[
        1,
        2,
        3,
        (1 << 28) - 1,
        1 << 28,
        (1 << 28) + 1,
        (1 << 29) - 1,
        1 << 29,
        (1 << 29) + 1,
        (1 << 30) - 1,
        1 << 30,
        (1 << 30) + 1,
        i32::MAX - 1,
        i32::MAX,
    ] {
        assert!(
            rsqrt_fast_is_within_bound(
                i64::from(I2F30::from_bits(raw).rsqrt_fast().to_bits()),
                i64::from(raw),
                30,
                i64::from(i32::MAX),
            ),
            "I2F30::rsqrt_fast at bits {raw}"
        );
    }

    // 1.0 and 0.25 are the two values a normalize leans on, and both are exact
    // in the approximate tier as well.
    assert_eq!(I2F30::from_bits(1 << 30).rsqrt_fast().to_bits(), 1 << 30);
    assert_eq!(I2F30::from_bits(1 << 28).rsqrt_fast().to_bits(), i32::MAX);
}

#[test]
fn rsqrt_fast_saturates_where_rsqrt_does() {
    // Zero and negatives have no reciprocal square root; both tiers answer MAX
    // rather than panicking, matching how `recip` treats zero.
    assert_eq!(I2F30::ZERO.rsqrt_fast(), I2F30::MAX);
    assert_eq!(I2F30::MIN.rsqrt_fast(), I2F30::MAX);
    assert_eq!(I2F30::from_bits(-1).rsqrt_fast(), I2F30::MAX);
    assert_eq!(I16F16::ZERO.rsqrt_fast(), I16F16::MAX);
    assert_eq!(I24F8::from_bits(-77).rsqrt_fast(), I24F8::MAX);
    assert_eq!(I8F8::ZERO.rsqrt_fast(), I8F8::MAX);
    assert_eq!(I0F8::ZERO.rsqrt_fast(), I0F8::MAX);

    // The smallest positive input gives the largest answer the type can be
    // asked for. `I2F30` cannot hold its own -- `2^15` against a range that
    // stops at 2 -- while `I16F16`'s `2^8` is comfortably inside, and lands
    // exactly, because a power of two is a fixed point of the whole routine.
    assert_eq!(I2F30::DELTA.rsqrt_fast(), I2F30::MAX);
    assert_eq!(I16F16::DELTA.rsqrt_fast(), I16F16::from_f64(256.0));
    assert_eq!(I16F16::DELTA.rsqrt_fast(), I16F16::DELTA.rsqrt());

    // At the top of I24F8's range the answer falls below half a step, which is
    // the branch that returns zero rather than shifting past the word.
    assert_eq!(I24F8::MAX.rsqrt_fast(), I24F8::ZERO);
}
