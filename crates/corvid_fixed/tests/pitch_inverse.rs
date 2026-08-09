//! The inverse trigonometry built on the clamping angles.
//!
//! `asin` and `acos` return a pitch because a pitch is exactly their range, so
//! what is checked is that the answer lands in range without wrapping, that it
//! inverts the sine exhaustively at 16 bits, and that the two are complements.

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
    reason = "these tests feed edge-case bit patterns through narrowing casts on purpose"
)]
mod common;

use common::Worst;
use corvid_fixed::{
    Angle16, Factor16, I24F8, Pitch8, Pitch16, Pitch32, Signed8, Signed16, Signed32,
};
#[test]
fn inverse_trigonometry_lands_in_range_without_wrapping() {
    // The phase comes back signed, so the negative half needs no wrapping
    // reinterpretation on the way into a pitch. Check the raw bits, not just
    // the clamped reading, or a wrapped value would hide behind `canonicalize`.
    let in_range =
        |p: Pitch16| p.to_bits() >= Pitch16::MIN.to_bits() && p.to_bits() <= Pitch16::MAX.to_bits();
    for bits in i16::MIN..=i16::MAX {
        let p = Pitch16::asin(Signed16::from_bits(bits));
        assert!(in_range(p), "asin({bits}) stored {}", p.to_bits());
        assert!(!p.is_out_of_range());
    }
    for bits in -128_i8..=127 {
        let p = Pitch8::asin(Signed8::from_bits(bits));
        assert!(
            p.to_bits() >= Pitch8::MIN.to_bits() && p.to_bits() <= Pitch8::MAX.to_bits(),
            "asin8({bits}) stored {}",
            p.to_bits()
        );
    }

    // Negative arcsines are genuinely negative in storage, not a large phase.
    assert_eq!(Pitch16::asin(Signed16::MIN).to_bits(), -16_384);
    assert_eq!(Pitch8::asin(Signed8::MIN).to_bits(), -64);
    assert_eq!(Pitch32::asin(Signed32::MIN).to_bits(), -1_073_741_824);
    assert!(Pitch16::asin(Signed16::from_f64(-0.5)).to_bits() < 0);

    // The arctangent too, over both half planes and every quadrant of input.
    for y in -300_i64..=300 {
        for x in [-997_i64, -1, 0, 1, 997] {
            let p = Pitch16::atan2(y, x);
            assert!(in_range(p), "atan2({y}, {x}) stored {}", p.to_bits());
            assert_eq!(p.is_negative(), y < 0, "sign flipped at ({y}, {x})");
        }
    }
    assert_eq!(Pitch16::atan2(-1, 0).to_bits(), -16_384);
    assert_eq!(Pitch32::atan2(-1, 0).to_bits(), -1_073_741_824);
    assert_eq!(Pitch8::atan2(-1, 0).to_bits(), -64);
}

#[test]
fn arcsine_inverts_sine_exhaustively_for_16_bit() {
    // Round-tripping a pitch through its own sine must return it, to within the
    // resolution the sine had to squeeze it through.
    let mut worst = Worst::default();
    for bits in -16_384_i16..=16_384 {
        let pitch = Pitch16::from_bits(bits);
        let recovered = Pitch16::asin(pitch.sin());
        let error = i128::from(recovered.to_bits()) - i128::from(bits);
        worst.observe(i128::from(bits), error.abs(), 0);
    }
    // Near the poles the sine flattens out, so many pitches share one sine and
    // the inverse cannot tell them apart. That is the sine's information loss,
    // not an inaccuracy in the arcsine.
    worst.assert_within(105, "Pitch16::asin round trip");
}

#[test]
fn arcsine_matches_the_reference() {
    let mut worst = Worst::default();
    for bits in i16::MIN..=i16::MAX {
        let value = Signed16::from_bits(bits);
        let expected = value.to_f64().asin() / core::f64::consts::TAU * 65_536.0;
        let actual = f64::from(Pitch16::asin(value).to_bits());
        worst.observe(i128::from(bits), (actual - expected).round() as i128, 0);
    }
    worst.assert_within(1, "Pitch16::asin against f64");
}

#[test]
fn arcsine_is_exact_at_the_endpoints_and_odd_about_zero() {
    assert_eq!(Pitch16::asin(Signed16::MAX), Pitch16::MAX);
    assert_eq!(Pitch16::asin(Signed16::MIN), Pitch16::MIN);
    assert_eq!(Pitch16::asin(Signed16::ZERO), Pitch16::ZERO);
    assert_eq!(Pitch8::asin(Signed8::MAX), Pitch8::MAX);
    assert_eq!(Pitch32::asin(Signed32::MIN), Pitch32::MIN);
    // The denormal encoding of -1.0 must behave like the canonical one.
    assert_eq!(Pitch16::asin(Signed16::from_bits(i16::MIN)), Pitch16::MIN);

    for bits in -32_767_i16..=32_767 {
        let value = Signed16::from_bits(bits);
        assert_eq!(
            Pitch16::asin(-value),
            -Pitch16::asin(value),
            "asin at {bits}"
        );
    }
}

#[test]
fn arccosine_is_the_complement_of_arcsine() {
    for bits in -32_767_i16..=32_767 {
        let value = Signed16::from_bits(bits);
        let arccosine = Angle16::acos(value);
        let arcsine = Pitch16::asin(value);
        assert_eq!(
            arccosine,
            Angle16::QUARTER_TURN - arcsine.to_angle(),
            "acos and asin disagree at {bits}"
        );
        // Always in the upper half of the circle.
        assert!(
            arccosine <= Angle16::HALF_TURN,
            "acos left [0, pi] at {bits}"
        );
    }

    assert_eq!(Angle16::acos(Signed16::MAX), Angle16::ZERO);
    assert_eq!(Angle16::acos(Signed16::ZERO), Angle16::QUARTER_TURN);
    assert_eq!(Angle16::acos(Signed16::MIN), Angle16::HALF_TURN);
}

#[test]
fn arccosine_matches_the_reference() {
    let mut worst = Worst::default();
    for bits in -32_767_i16..=32_767 {
        let value = Signed16::from_bits(bits);
        let expected = value.to_f64().acos() / core::f64::consts::TAU * 65_536.0;
        let actual = f64::from(Angle16::acos(value).to_bits());
        worst.observe(i128::from(bits), (actual - expected).round() as i128, 0);
    }
    worst.assert_within(1, "Angle16::acos against f64");
}

#[test]
fn arctangent_folds_onto_the_right_half_plane() {
    assert_eq!(Pitch16::atan2(0, 1), Pitch16::ZERO);
    assert_eq!(Pitch16::atan2(1, 1), Pitch16::from_degrees(45.0));
    assert_eq!(Pitch16::atan2(-1, 1), Pitch16::from_degrees(-45.0));
    // A negative x mirrors instead of turning past vertical.
    assert_eq!(Pitch16::atan2(1, -1), Pitch16::from_degrees(45.0));
    assert_eq!(Pitch16::atan2(1, 0), Pitch16::MAX);
    assert_eq!(Pitch16::atan2(-1, 0), Pitch16::MIN);
    assert_eq!(Pitch16::atan2(0, 0), Pitch16::ZERO);

    // Scale invariant, and never leaves the range.
    for y in -20_i64..=20 {
        for x in -20_i64..=20 {
            let pitch = Pitch16::atan2(y, x);
            assert!(!pitch.is_out_of_range(), "atan2({y}, {x}) left the range");
            assert_eq!(
                pitch,
                Pitch16::atan2(y * 997, x * 997),
                "not scale invariant"
            );
        }
    }
}

#[test]
fn arctangent_survives_extreme_coordinates() {
    // i64::MIN has no positive counterpart, so mirroring it must not overflow.
    assert_eq!(
        Pitch16::atan2(i64::MAX, i64::MIN),
        Pitch16::from_degrees(45.0)
    );
    assert_eq!(
        Pitch16::atan2(i64::MIN, i64::MIN),
        Pitch16::from_degrees(-45.0)
    );
    assert_eq!(Pitch16::atan2(i64::MIN, 0), Pitch16::MIN);
    assert_eq!(Pitch16::atan2(0, i64::MIN), Pitch16::ZERO);
    assert_eq!(
        Pitch32::atan2(i64::MIN, i64::MIN),
        Pitch32::from_degrees(-45.0)
    );
}

#[test]
fn arcsine_is_correct_at_the_other_widths() {
    // Exhaustive for 8-bit, which exercises the same code path the 16-bit tests
    // cover, and sampled for 32-bit, which takes the 128-bit square root branch.
    for bits in -127_i8..=127 {
        let value = Signed8::from_bits(bits);
        let expected = value.to_f64().asin() / core::f64::consts::TAU * 256.0;
        let actual = f64::from(Pitch8::asin(value).to_bits());
        assert!((actual - expected).abs() <= 1.0, "Pitch8::asin at {bits}");
    }

    for step in 0..=2000_i32 {
        let bits = ((i64::from(step) * 2_147_483_647) / 1000 - 2_147_483_647) as i32;
        let value = Signed32::from_bits(bits.clamp(-2_147_483_647, 2_147_483_647));
        let expected = value.to_f64().asin() / core::f64::consts::TAU * 4_294_967_296.0;
        let actual = f64::from(Pitch32::asin(value).to_bits());
        assert!(
            (actual - expected).abs() <= 2.0,
            "Pitch32::asin at {bits}: {actual} vs {expected}"
        );
    }
}

#[test]
fn interpolation_hits_both_endpoints() {
    let low = Pitch16::from_degrees(-60.0);
    let high = Pitch16::from_degrees(60.0);
    assert_eq!(low.lerp(high, Factor16::ZERO), low);
    assert_eq!(low.lerp(high, Factor16::ONE), high);
    assert_eq!(low.lerp(high, Factor16::from_f64(0.5)), Pitch16::ZERO);
    // No short way around, unlike the wrapping angles.
    assert_eq!(
        Pitch16::MIN.lerp(Pitch16::MAX, Factor16::from_f64(0.5)),
        Pitch16::ZERO
    );
}

#[test]
fn ordering_runs_from_down_to_up() {
    assert!(Pitch16::MIN < Pitch16::ZERO);
    assert!(Pitch16::ZERO < Pitch16::MAX);
    assert_eq!(Pitch16::MIN.max(Pitch16::MAX), Pitch16::MAX);
    assert_eq!(
        Pitch16::ZERO.clamp(Pitch16::MIN, Pitch16::MAX),
        Pitch16::ZERO
    );
    assert!(Pitch16::MAX.is_positive());
    assert!(Pitch16::MIN.is_negative());
    assert!(!Pitch16::ZERO.is_positive());
    assert!(!Pitch16::ZERO.is_negative());

    // Out-of-range bits compare as their clamped value, not their raw one.
    assert!(Pitch16::from_bits(30_000) <= Pitch16::MAX);
}

#[test]
fn display_reads_as_turns() {
    assert_eq!(format!("{:?}", Pitch16::MAX), "Pitch16(0.25 turn)");
    assert_eq!(Pitch16::MAX.to_string(), "0.25");
    assert_eq!(format!("{:?}", Pitch16::MIN), "Pitch16(-0.25 turn)");
}

#[test]
fn everything_is_available_in_const_context() {
    const PITCH: Pitch16 = Pitch16::from_degrees(30.0);
    const CLAMPED: Pitch16 = Pitch16::from_degrees(120.0);
    const SIN: Signed16 = PITCH.sin();
    const TAN: I24F8 = PITCH.tan();
    const ARCSINE: Pitch16 = Pitch16::asin(Signed16::MAX);
    const ARCCOS: Angle16 = Angle16::acos(Signed16::ZERO);
    const ARCTAN: Pitch16 = Pitch16::atan2(3, 4);
    const SUM: Pitch16 = PITCH.saturating_add(Pitch16::MAX);
    const AS_ANGLE: Angle16 = PITCH.to_angle();

    assert_eq!(CLAMPED, Pitch16::MAX);
    assert!((SIN.to_f64() - 0.5).abs() < 1e-4);
    assert!((TAN.to_f64() - 0.5774).abs() < 1e-2);
    assert_eq!(ARCSINE, Pitch16::MAX);
    assert_eq!(ARCCOS, Angle16::QUARTER_TURN);
    assert!((ARCTAN.to_degrees() - 36.87).abs() < 0.1);
    assert_eq!(SUM, Pitch16::MAX);
    assert_eq!(AS_ANGLE, Angle16::from_degrees(30.0));
}
