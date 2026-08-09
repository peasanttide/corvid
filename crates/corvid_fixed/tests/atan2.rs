//! The tangent and the arctangent, against `f64` and against each other.
//!
//! `atan2` is the one function here whose domain is two coordinates rather than
//! a phase, so what it owes is a grid, the axes, the origin, scale invariance,
//! and the extreme coordinates a naive implementation overflows on.

#![allow(
    clippy::panic_in_result_fn,
    clippy::missing_panics_doc,
    clippy::float_cmp,
    reason = "tests assert; a panic is how a test reports failure"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::suboptimal_flops,
    reason = "these tests feed edge-case bit patterns through narrowing casts on purpose"
)]
mod common;

use common::Worst;
use corvid_fixed::{Angle16, Angle32, I24F8, Signed16};

#[test]
fn tangent_matches_the_reference_away_from_the_poles() {
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        let expected = angle.to_radians().tan();
        if !(-1000.0..=1000.0).contains(&expected) {
            continue;
        }
        let actual = angle.tan().to_f64();
        // I24F8 resolves to 1/256; the tangent's slope multiplies that up.
        let tolerance = 1.0_f64.max(expected.abs()) * 0.01 + 0.004;
        assert!(
            (actual - expected).abs() < tolerance,
            "tan at {bits}: {actual} vs {expected}"
        );
    }
}

#[test]
fn tangent_saturates_at_the_poles() {
    assert_eq!(Angle16::QUARTER_TURN.tan(), I24F8::MAX);
    assert_eq!(Angle16::THREE_QUARTER_TURN.tan(), I24F8::MIN);
    assert_eq!(Angle16::ZERO.tan(), I24F8::ZERO);
    assert_eq!(Angle16::HALF_TURN.tan(), I24F8::ZERO);
    assert_eq!(Angle32::QUARTER_TURN.tan(), I24F8::MAX);
}

#[test]
fn tangent_is_the_ratio_of_sine_to_cosine() {
    // An eighth of a turn is where both are equal, so the tangent is one.
    let eighth = Angle16::from_bits(8192);
    assert_eq!(eighth.tan(), I24F8::ONE);
    assert_eq!((-Angle16::from_bits(8192)).tan(), -I24F8::ONE);
}

#[test]
fn atan2_inverts_sin_cos() {
    // Round-tripping an angle through its own sine and cosine must return it.
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        let (sin, cos) = angle.sin_cos();
        let recovered = Angle16::atan2(i64::from(sin.to_bits()), i64::from(cos.to_bits()));
        let error = angle.abs_diff(recovered).to_bits();
        assert!(error <= 1, "atan2 round-trip off by {error} bits at {bits}");
    }
}

#[test]
fn atan2_matches_the_reference_over_a_grid() {
    let mut worst = Worst::default();
    for y in -64_i64..=64 {
        for x in -64_i64..=64 {
            if x == 0 && y == 0 {
                continue;
            }
            let expected = (y as f64).atan2(x as f64) / core::f64::consts::TAU;
            let expected_bits = (expected.rem_euclid(1.0) * 65_536.0).round() as i128 % 65_536;
            let actual = i128::from(Angle16::atan2(y, x).to_bits());
            // Compare on the circle: 0 and 65535 are one bit apart.
            let direct = (actual - expected_bits).abs();
            let wrapped = 65_536 - direct;
            worst.observe(i128::from(y * 1000 + x), direct.min(wrapped), 0);
        }
    }
    worst.assert_within(0, "Angle16::atan2 over a grid");
}

#[test]
fn atan2_handles_the_axes_and_the_origin() {
    assert_eq!(Angle16::atan2(0, 0), Angle16::ZERO);
    assert_eq!(Angle16::atan2(0, 5), Angle16::ZERO);
    assert_eq!(Angle16::atan2(5, 0), Angle16::QUARTER_TURN);
    assert_eq!(Angle16::atan2(0, -5), Angle16::HALF_TURN);
    assert_eq!(Angle16::atan2(-5, 0), Angle16::THREE_QUARTER_TURN);

    assert_eq!(Angle16::atan2(1, 1), Angle16::from_degrees(45.0));
    assert_eq!(Angle16::atan2(1, -1), Angle16::from_degrees(135.0));
    assert_eq!(Angle16::atan2(-1, -1), Angle16::from_degrees(225.0));
    assert_eq!(Angle16::atan2(-1, 1), Angle16::from_degrees(315.0));
}

#[test]
fn atan2_is_scale_invariant() {
    let base = Angle32::atan2(3, 7);
    for scale in [1_i64, 2, 17, 1024, 1_000_000, 1_000_000_000] {
        let scaled = Angle32::atan2(3 * scale, 7 * scale);
        let error = base.abs_diff(scaled).to_bits();
        assert!(error <= 2, "scale {scale} moved the angle by {error} bits");
    }
}

#[test]
fn atan2_survives_extreme_coordinates() {
    // No overflow, no panic, and the quadrant is still right.
    assert_eq!(
        Angle16::atan2(i64::MAX, i64::MAX),
        Angle16::from_degrees(45.0)
    );
    assert_eq!(
        Angle16::atan2(i64::MIN, i64::MIN),
        Angle16::from_degrees(225.0)
    );
    assert_eq!(Angle16::atan2(0, i64::MIN), Angle16::HALF_TURN);
    assert_eq!(Angle16::atan2(1, i64::MAX), Angle16::ZERO);
}

#[test]
fn trigonometry_is_available_in_const_context() {
    const HEADING: Angle16 = Angle16::from_degrees(120.0);
    const SIN: Signed16 = HEADING.sin();
    const COS: Signed16 = HEADING.cos();
    const TAN: I24F8 = HEADING.tan();
    const FAST: Signed16 = HEADING.sin_fast();
    const BACK: Angle16 = Angle16::atan2(1, -1);
    const BACK_FAST: Angle16 = Angle16::atan2_fast(1, -1);

    assert!((SIN.to_f64() - 0.866).abs() < 1e-3);
    assert!((COS.to_f64() + 0.5).abs() < 1e-3);
    assert!((TAN.to_f64() + 1.732).abs() < 1e-2);
    assert!((FAST.to_f64() - 0.866).abs() < 2e-3);
    assert_eq!(BACK, Angle16::from_degrees(135.0));
    assert!(BACK_FAST.abs_diff(BACK).to_bits() < 100);
}
