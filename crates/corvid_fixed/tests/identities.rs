//! The identities: what multiplying or dividing by one leaves alone, what a
//! complement comes to, and where an interpolation lands.
//!
//! An identity is the cheapest thing to get wrong without noticing, because
//! every value is its own reference and nothing outside the crate has to be
//! consulted to say what the answer should be.

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

use common::Rng;
use corvid_fixed::{
    Angle8, Angle16, Angle32, Factor8, Factor16, Factor32, I8F8, I24F8, Signed8, Signed16,
};

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
