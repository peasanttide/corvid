//! Verifies the trigonometry against `f64`, exhaustively where the domain
//! allows.
//!
//! [`Angle8`] and [`Angle16`] have 256 and 65536 possible inputs, so every
//! result is checked against a reference computed in `f64`. [`Angle32`] is
//! sampled at boundaries plus a deterministic sweep. Errors are measured in
//! units of the output type's last bit, and the asserted limits are regression
//! bounds: they are what the implementation currently achieves, so any loss of
//! accuracy shows up as a failure rather than a silent drift.

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

use common::{Rng, Worst};
use corvid_fixed::{Angle8, Angle16, Angle32, I24F8, Signed8, Signed16, Signed32};

/// The reference sine of a phase, as the output type's nearest bit pattern.
fn reference(phase: f64, turn: f64, scale: f64, quarter_offset: f64) -> i128 {
    let radians = (phase / turn + quarter_offset) * core::f64::consts::TAU;
    (radians.sin() * scale).round() as i128
}

#[test]
fn sin_and_cos_are_exhaustively_correctly_rounded_for_angle8() {
    let scale = f64::from(Signed8::MAX.to_bits());
    let mut sin = Worst::default();
    let mut cos = Worst::default();

    for bits in 0..=u8::MAX {
        let angle = Angle8::from_bits(bits);
        let phase = f64::from(bits);
        sin.observe(
            i128::from(bits),
            i128::from(angle.sin().to_bits()),
            reference(phase, 256.0, scale, 0.0),
        );
        cos.observe(
            i128::from(bits),
            i128::from(angle.cos().to_bits()),
            reference(phase, 256.0, scale, 0.25),
        );
    }

    assert_eq!(sin.checked, 256);
    sin.assert_within(0, "Angle8::sin");
    cos.assert_within(0, "Angle8::cos");
}

#[test]
fn sin_and_cos_are_exhaustively_correctly_rounded_for_angle16() {
    let scale = f64::from(Signed16::MAX.to_bits());
    let mut sin = Worst::default();
    let mut cos = Worst::default();

    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        let phase = f64::from(bits);
        sin.observe(
            i128::from(bits),
            i128::from(angle.sin().to_bits()),
            reference(phase, 65_536.0, scale, 0.0),
        );
        cos.observe(
            i128::from(bits),
            i128::from(angle.cos().to_bits()),
            reference(phase, 65_536.0, scale, 0.25),
        );
    }

    assert_eq!(sin.checked, 65_536);
    sin.assert_within(0, "Angle16::sin");
    cos.assert_within(0, "Angle16::cos");
}

#[test]
fn sin_and_cos_are_within_one_bit_for_angle32() {
    let scale = f64::from(Signed32::MAX.to_bits());
    let turn = 4_294_967_296.0;
    let mut sin = Worst::default();
    let mut cos = Worst::default();
    let mut rng = Rng::new(0x5eed_1234);

    // Every boundary the octant folding can land on, plus a wide sweep.
    let mut phases = vec![0_u32, 1, u32::MAX];
    for octant in 0..8_u32 {
        let base = octant << 29;
        phases.extend([base.wrapping_sub(1), base, base + 1]);
    }
    phases.extend((0..20_000).map(|_| rng.next_u32()));

    for phase in phases {
        let angle = Angle32::from_bits(phase);
        let reference_phase = f64::from(phase);
        sin.observe(
            i128::from(phase),
            i128::from(angle.sin().to_bits()),
            reference(reference_phase, turn, scale, 0.0),
        );
        cos.observe(
            i128::from(phase),
            i128::from(angle.cos().to_bits()),
            reference(reference_phase, turn, scale, 0.25),
        );
    }

    // The narrower widths are held to exact rounding. Here the tolerance is one
    // bit, not because the implementation needs it — it passes at zero — but
    // because the f64 reference does: Signed32's last bit is 4.7e-10, close
    // enough to f64's own error that a different libm could round a near-tie
    // the other way and fail a test that is not testing this crate.
    sin.assert_within(1, "Angle32::sin");
    cos.assert_within(1, "Angle32::cos");
}

#[test]
fn quarter_turns_are_exact() {
    assert_eq!(Angle16::ZERO.sin(), Signed16::ZERO);
    assert_eq!(Angle16::ZERO.cos(), Signed16::MAX);
    assert_eq!(Angle16::QUARTER_TURN.sin(), Signed16::MAX);
    assert_eq!(Angle16::QUARTER_TURN.cos(), Signed16::ZERO);
    assert_eq!(Angle16::HALF_TURN.sin(), Signed16::ZERO);
    assert_eq!(Angle16::HALF_TURN.cos(), Signed16::MIN);
    assert_eq!(Angle16::THREE_QUARTER_TURN.sin(), Signed16::MIN);
    assert_eq!(Angle16::THREE_QUARTER_TURN.cos(), Signed16::ZERO);

    assert_eq!(Angle8::QUARTER_TURN.sin(), Signed8::MAX);
    assert_eq!(Angle32::QUARTER_TURN.sin(), Signed32::MAX);
    assert_eq!(Angle32::HALF_TURN.cos(), Signed32::MIN);
}

#[test]
fn sine_is_odd_and_cosine_is_even() {
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        let mirrored = -angle;
        assert_eq!(mirrored.sin(), -angle.sin(), "sin(-x) != -sin(x) at {bits}");
        assert_eq!(mirrored.cos(), angle.cos(), "cos(-x) != cos(x) at {bits}");
    }
}

#[test]
fn pythagorean_identity_holds() {
    // sin^2 + cos^2 = 1, to within the rounding of two squarings.
    for bits in 0..=u16::MAX {
        let (sin, cos) = Angle16::from_bits(bits).sin_cos();
        let sum = f64::from(sin.to_bits()).powi(2) + f64::from(cos.to_bits()).powi(2);
        let unit = f64::from(Signed16::MAX.to_bits()).powi(2);
        let error = (sum / unit - 1.0).abs();
        assert!(error < 1e-4, "sin^2 + cos^2 off by {error:e} at {bits}");
    }
}

#[test]
fn sin_cos_agrees_with_the_separate_calls() {
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        assert_eq!(angle.sin_cos(), (angle.sin(), angle.cos()));
    }
}

#[test]
fn cosine_leads_sine_by_a_quarter_turn() {
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        assert_eq!(angle.cos(), (angle + Angle16::QUARTER_TURN).sin());
    }
}

#[test]
fn fast_sine_is_within_its_documented_error() {
    let scale = f64::from(Signed16::MAX.to_bits());
    let mut worst = Worst::default();
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        worst.observe(
            i128::from(bits),
            i128::from(angle.sin_fast().to_bits()),
            reference(f64::from(bits), 65_536.0, scale, 0.0),
        );
    }
    // 1.1e-3 of full scale, in units of Signed16's last bit.
    let limit = (1.1e-3 * scale).ceil() as i128;
    worst.assert_within(limit, "Angle16::sin_fast");

    // At 8-bit output the approximation is already exact to the last bit.
    let scale8 = f64::from(Signed8::MAX.to_bits());
    let mut worst8 = Worst::default();
    for bits in 0..=u8::MAX {
        worst8.observe(
            i128::from(bits),
            i128::from(Angle8::from_bits(bits).sin_fast().to_bits()),
            reference(f64::from(bits), 256.0, scale8, 0.0),
        );
    }
    worst8.assert_within(1, "Angle8::sin_fast");
}

#[test]
fn fast_trigonometry_is_exact_at_the_quarter_turns() {
    assert_eq!(Angle16::ZERO.sin_fast(), Signed16::ZERO);
    assert_eq!(Angle16::QUARTER_TURN.sin_fast(), Signed16::MAX);
    assert_eq!(Angle16::HALF_TURN.sin_fast(), Signed16::ZERO);
    assert_eq!(Angle16::THREE_QUARTER_TURN.sin_fast(), Signed16::MIN);

    assert_eq!(Angle16::ZERO.cos_fast(), Signed16::MAX);
    assert_eq!(Angle16::QUARTER_TURN.cos_fast(), Signed16::ZERO);
    assert_eq!(Angle16::HALF_TURN.cos_fast(), Signed16::MIN);
}

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
fn fast_atan2_is_within_its_documented_error() {
    // 4.4e-3 radians, expressed in Angle16 bits.
    let limit = (4.4e-3 / core::f64::consts::TAU * 65_536.0).ceil() as u16;
    let mut worst = 0_u16;
    for y in -40_i64..=40 {
        for x in -40_i64..=40 {
            let exact = Angle16::atan2(y, x);
            let fast = Angle16::atan2_fast(y, x);
            worst = worst.max(exact.abs_diff(fast).to_bits());
        }
    }
    assert!(
        worst <= limit,
        "fast atan2 off by {worst} bits (limit {limit})"
    );
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
