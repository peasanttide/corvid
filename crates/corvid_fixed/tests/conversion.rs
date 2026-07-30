//! Float conversion: round-trip fidelity, rounding, saturation, and the
//! handling of `NaN` and infinities.
//!
//! The 8-bit and 16-bit types are walked exhaustively. The 32-bit types are
//! checked at every boundary plus a deterministic sample.

#![allow(
    clippy::float_cmp,
    reason = "these tests are about exact float values, so exact comparison is the point"
)]

mod common;

use common::Rng;
use corvid_fixed::{
    Angle8, Angle16, Angle32, Factor8, Factor16, Factor32, I0F8, I8F8, I24F8, Signed8, Signed16,
    Signed32,
};

/// Asserts that `bits -> f64 -> bits` is the identity across a whole type.
macro_rules! assert_f64_round_trip_exhaustive {
    ($name:ident, $repr:ty) => {
        for bits in <$repr>::MIN..=<$repr>::MAX {
            let value = $name::from_bits(bits);
            let round_tripped = $name::from_f64(value.to_f64());
            assert_eq!(
                round_tripped, value,
                concat!(stringify!($name), " lost {} through f64"),
                bits
            );
        }
    };
}

/// Asserts that `bits -> f32 -> bits` is the identity across a whole type.
macro_rules! assert_f32_round_trip_exhaustive {
    ($name:ident, $repr:ty) => {
        for bits in <$repr>::MIN..=<$repr>::MAX {
            let value = $name::from_bits(bits);
            let round_tripped = $name::from_f32(value.to_f32());
            assert_eq!(
                round_tripped, value,
                concat!(stringify!($name), " lost {} through f32"),
                bits
            );
        }
    };
}

#[test]
fn every_8_and_16_bit_value_round_trips_through_f64() {
    assert_f64_round_trip_exhaustive!(I0F8, i8);
    assert_f64_round_trip_exhaustive!(I8F8, i16);
    assert_f64_round_trip_exhaustive!(Factor8, u8);
    assert_f64_round_trip_exhaustive!(Factor16, u16);
    assert_f64_round_trip_exhaustive!(Signed8, i8);
    assert_f64_round_trip_exhaustive!(Signed16, i16);
    assert_f64_round_trip_exhaustive!(Angle8, u8);
    assert_f64_round_trip_exhaustive!(Angle16, u16);
}

#[test]
fn every_8_and_16_bit_value_round_trips_through_f32() {
    // 24 mantissa bits is enough for anything 16 bits wide.
    assert_f32_round_trip_exhaustive!(I0F8, i8);
    assert_f32_round_trip_exhaustive!(I8F8, i16);
    assert_f32_round_trip_exhaustive!(Factor8, u8);
    assert_f32_round_trip_exhaustive!(Factor16, u16);
    assert_f32_round_trip_exhaustive!(Signed8, i8);
    assert_f32_round_trip_exhaustive!(Signed16, i16);
    assert_f32_round_trip_exhaustive!(Angle8, u8);
    assert_f32_round_trip_exhaustive!(Angle16, u16);
}

#[test]
fn the_denormal_snorm_encoding_round_trips_to_the_canonical_one() {
    // -128 and -127 are both -1.0, so f64 cannot tell them apart; the round-trip
    // lands on the canonical encoding, and they compare equal either way.
    assert_eq!(Signed8::from_f64(Signed8::from_bits(-128).to_f64()), Signed8::MIN);
    assert_eq!(Signed8::from_bits(-128), Signed8::from_bits(-127));
    assert_eq!(Signed16::from_bits(i16::MIN), Signed16::MIN);
    assert_eq!(Signed32::from_bits(i32::MIN), Signed32::MIN);
}

#[test]
fn the_32_bit_types_round_trip_through_f64() {
    let mut rng = Rng::new(0xc0ff_ee11);
    let mut samples: Vec<u32> = vec![0, 1, 2, u32::MAX, u32::MAX - 1, 1 << 31, (1 << 31) - 1];
    samples.extend((0..50_000).map(|_| rng.next_u32()));

    for raw in samples {
        let signed = raw as i32;

        let fixed = I24F8::from_bits(signed);
        assert_eq!(I24F8::from_f64(fixed.to_f64()), fixed, "I24F8 lost {signed}");

        let factor = Factor32::from_bits(raw);
        assert_eq!(Factor32::from_f64(factor.to_f64()), factor, "Factor32 lost {raw}");

        let snorm = Signed32::from_bits(signed).canonicalize();
        assert_eq!(Signed32::from_f64(snorm.to_f64()), snorm, "Signed32 lost {signed}");

        let angle = Angle32::from_bits(raw);
        assert_eq!(Angle32::from_turns(angle.to_turns()), angle, "Angle32 lost {raw}");
    }
}

#[test]
fn nan_converts_to_zero() {
    assert_eq!(I24F8::from_f64(f64::NAN), I24F8::ZERO);
    assert_eq!(I0F8::from_f64(f64::NAN), I0F8::ZERO);
    assert_eq!(Factor16::from_f64(f64::NAN), Factor16::ZERO);
    assert_eq!(Signed16::from_f64(f64::NAN), Signed16::ZERO);
    assert_eq!(Angle16::from_turns(f64::NAN), Angle16::ZERO);
    assert_eq!(I24F8::from_f32(f32::NAN), I24F8::ZERO);
}

#[test]
fn nan_is_rejected_by_the_checked_conversions() {
    assert_eq!(I24F8::checked_from_f64(f64::NAN), None);
    assert_eq!(Factor16::checked_from_f64(f64::NAN), None);
    assert_eq!(Signed16::checked_from_f64(f64::NAN), None);
    assert_eq!(Angle16::checked_from_f64(f64::NAN), None);
    assert_eq!(I8F8::checked_from_f32(f32::NAN), None);
}

#[test]
fn infinities_saturate() {
    assert_eq!(I24F8::from_f64(f64::INFINITY), I24F8::MAX);
    assert_eq!(I24F8::from_f64(f64::NEG_INFINITY), I24F8::MIN);
    assert_eq!(I0F8::from_f64(f64::INFINITY), I0F8::MAX);
    assert_eq!(Factor32::from_f64(f64::INFINITY), Factor32::MAX);
    assert_eq!(Factor32::from_f64(f64::NEG_INFINITY), Factor32::ZERO);
    assert_eq!(Signed16::from_f64(f64::INFINITY), Signed16::MAX);
    assert_eq!(Signed16::from_f64(f64::NEG_INFINITY), Signed16::MIN);
}

#[test]
fn infinities_are_rejected_by_the_checked_conversions() {
    assert_eq!(I24F8::checked_from_f64(f64::INFINITY), None);
    assert_eq!(Factor8::checked_from_f64(f64::NEG_INFINITY), None);
    assert_eq!(Signed32::checked_from_f64(f64::INFINITY), None);
}

#[test]
fn out_of_range_values_saturate_but_are_detectable() {
    assert_eq!(I8F8::from_f64(1e9), I8F8::MAX);
    assert_eq!(I8F8::from_f64(-1e9), I8F8::MIN);
    assert_eq!(I8F8::checked_from_f64(1e9), None);
    assert_eq!(I8F8::checked_from_f64(128.0), None);
    assert_eq!(I8F8::checked_from_f64(127.99609375), Some(I8F8::MAX));
    // Just under the point where rounding would carry past MAX.
    assert_eq!(I8F8::checked_from_f64(127.998), Some(I8F8::MAX));
    assert_eq!(I8F8::checked_from_f64(127.999), None);

    assert_eq!(Factor8::from_f64(1.5), Factor8::ONE);
    assert_eq!(Factor8::from_f64(-0.5), Factor8::ZERO);
    assert_eq!(Factor8::checked_from_f64(1.5), None);
    assert_eq!(Factor8::checked_from_f64(-0.5), None);
    assert_eq!(Factor8::checked_from_f64(1.0), Some(Factor8::ONE));
    assert_eq!(Factor8::checked_from_f64(0.0), Some(Factor8::ZERO));

    assert_eq!(Signed8::from_f64(2.0), Signed8::MAX);
    assert_eq!(Signed8::from_f64(-2.0), Signed8::MIN);
    assert_eq!(Signed8::checked_from_f64(-1.0), Some(Signed8::MIN));
    assert_eq!(Signed8::checked_from_f64(1.0), Some(Signed8::MAX));
    // Half a step of Signed8 is 1/254, so 1.002 still rounds onto MAX and
    // 1.005 does not.
    assert_eq!(Signed8::checked_from_f64(1.002), Some(Signed8::MAX));
    assert_eq!(Signed8::checked_from_f64(1.005), None);
    assert_eq!(Signed8::checked_from_f64(-1.005), None);
}

#[test]
fn conversion_rounds_halfway_away_from_zero() {
    // 1/512 is exactly half of I8F8's resolution.
    let half_step = 1.0 / 512.0;
    assert_eq!(I8F8::from_f64(half_step).to_bits(), 1);
    assert_eq!(I8F8::from_f64(-half_step).to_bits(), -1);
    assert_eq!(I8F8::from_f64(half_step * 0.99).to_bits(), 0);
    assert_eq!(I8F8::from_f64(-half_step * 0.99).to_bits(), 0);
    assert_eq!(I8F8::from_f64(3.0 * half_step).to_bits(), 2);

    // Angles round the same way.
    assert_eq!(Angle8::from_turns(1.0 / 512.0).to_bits(), 1);
    assert_eq!(Angle8::from_turns(-1.0 / 512.0).to_bits(), 255);
}

#[test]
fn exact_powers_of_two_are_exact_in_the_fixed_point_family() {
    for exponent in -8_i32..=6 {
        let value = 2.0_f64.powi(exponent);
        assert_eq!(I8F8::from_f64(value).to_f64(), value, "2^{exponent}");
        assert_eq!(I24F8::from_f64(value).to_f64(), value, "2^{exponent}");
    }
    assert_eq!(I8F8::from_f64(0.00390625).to_bits(), 1);
    assert_eq!(I0F8::from_f64(0.25).to_bits(), 64);
}

#[test]
fn the_unorm_and_snorm_endpoints_are_exact() {
    assert_eq!(Factor8::ONE.to_f64(), 1.0);
    assert_eq!(Factor16::ONE.to_f64(), 1.0);
    assert_eq!(Factor32::ONE.to_f64(), 1.0);
    assert_eq!(Factor8::ZERO.to_f64(), 0.0);

    assert_eq!(Signed8::MAX.to_f64(), 1.0);
    assert_eq!(Signed16::MAX.to_f64(), 1.0);
    assert_eq!(Signed32::MAX.to_f64(), 1.0);
    assert_eq!(Signed8::MIN.to_f64(), -1.0);
    assert_eq!(Signed16::MIN.to_f64(), -1.0);
    assert_eq!(Signed32::MIN.to_f64(), -1.0);
    assert_eq!(Signed16::ZERO.to_f64(), 0.0);
}

#[test]
fn angle_units_agree_with_each_other() {
    for degrees in [0.0, 45.0, 90.0, 123.75, 180.0, 270.0, 359.0] {
        let from_degrees = Angle16::from_degrees(degrees);
        let from_turns = Angle16::from_turns(degrees / 360.0);
        let from_radians = Angle16::from_radians(degrees.to_radians());
        assert_eq!(from_degrees, from_turns, "{degrees} degrees");
        assert!(
            from_degrees.abs_diff(from_radians).to_bits() <= 1,
            "{degrees} degrees via radians"
        );
        assert!((from_degrees.to_degrees() - degrees).abs() < 0.01);
    }
}

#[test]
fn angles_wrap_rather_than_saturate() {
    assert_eq!(Angle16::from_degrees(360.0), Angle16::ZERO);
    assert_eq!(Angle16::from_degrees(720.0), Angle16::ZERO);
    assert_eq!(Angle16::from_degrees(450.0), Angle16::QUARTER_TURN);
    assert_eq!(Angle16::from_degrees(-90.0), Angle16::THREE_QUARTER_TURN);
    assert_eq!(Angle16::from_degrees(-450.0), Angle16::THREE_QUARTER_TURN);
    assert_eq!(Angle16::from_turns(1000.25), Angle16::QUARTER_TURN);
}

#[test]
fn signed_angle_readings_cover_the_negative_half() {
    assert_eq!(Angle16::ZERO.to_signed_turns(), 0.0);
    assert_eq!(Angle16::QUARTER_TURN.to_signed_turns(), 0.25);
    assert_eq!(Angle16::HALF_TURN.to_signed_turns(), -0.5);
    assert_eq!(Angle16::THREE_QUARTER_TURN.to_signed_turns(), -0.25);
    assert!((Angle16::THREE_QUARTER_TURN.to_signed_radians() + core::f64::consts::FRAC_PI_2).abs() < 1e-9);

    assert_eq!(Angle8::MAX.to_signed_bits(), -1);
    assert_eq!(Angle16::MAX.to_signed_bits(), -1);
    assert_eq!(Angle32::MAX.to_signed_bits(), -1);
}

#[test]
fn to_f32_rounds_once() {
    // Going through f64 first means a single rounding step. A value that needs
    // more than 24 bits proves the difference: naive f32 arithmetic on the bits
    // would round twice.
    let value = I24F8::from_bits(0x0055_5555);
    assert_eq!(value.to_f32(), value.to_f64() as f32);
    let factor = Factor32::from_bits(0xAAAA_AAAA);
    assert_eq!(factor.to_f32(), factor.to_f64() as f32);
}

#[test]
fn display_and_debug_read_as_numbers() {
    assert_eq!(I8F8::from_f64(1.5).to_string(), "1.5");
    assert_eq!(I8F8::from_f64(-0.25).to_string(), "-0.25");
    assert_eq!(format!("{:.2}", I8F8::from_f64(1.5)), "1.50");
    assert_eq!(format!("{:?}", I8F8::from_f64(1.5)), "I8F8(1.5)");
    assert_eq!(Factor8::ONE.to_string(), "1");
    assert_eq!(format!("{:?}", Factor8::ONE), "Factor8(1)");
    assert_eq!(format!("{:?}", Signed8::MIN), "Signed8(-1)");
    assert_eq!(format!("{:?}", Angle16::QUARTER_TURN), "Angle16(0.25 turn)");
    assert_eq!(Angle16::QUARTER_TURN.to_string(), "0.25");
}

#[test]
fn conversions_are_available_in_const_context() {
    const FIXED: I24F8 = I24F8::from_f64(-12.5);
    const FACTOR: Factor16 = Factor16::from_f32(0.5);
    const SNORM: Signed16 = Signed16::from_f64(-1.0);
    const ANGLE: Angle32 = Angle32::from_degrees(45.0);
    const BACK: f64 = FIXED.to_f64();
    const CHECKED: Option<I8F8> = I8F8::checked_from_f64(1000.0);

    assert_eq!(FIXED.to_bits(), -3200);
    assert_eq!(FACTOR.to_bits(), 32768);
    assert_eq!(SNORM, Signed16::MIN);
    assert_eq!(ANGLE.to_bits(), 1 << 29);
    assert_eq!(BACK, -12.5);
    assert_eq!(CHECKED, None);
}
