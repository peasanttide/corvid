//! The angle units, which convert by wrapping rather than by saturating.
//!
//! Turns, radians and degrees are three readings of the same stored phase, so
//! what is checked is that they agree with each other, that a value past a full
//! turn comes back inside one, and that the checked form rejects exactly what
//! needed wrapping and nothing else.

#![allow(
    clippy::float_cmp,
    reason = "these tests are about exact float values, so exact comparison is the point"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    reason = "these tests feed edge-case bit patterns through narrowing casts on purpose"
)]
use corvid_fixed::{
    Angle8, Angle16, Angle32, Factor8, Factor16, Factor32, I8F8, I24F8, Signed8, Signed16,
};
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
fn angles_wrap_at_any_turn_count() {
    // Whole turns are discarded before scaling, so a large turn count wraps
    // like a small one. Scaling first would push the intermediate past the i64
    // the conversion casts through, and the cast would saturate: the quarter
    // turn below came back as 0.99999999977 of a turn before that was fixed.
    assert_eq!(Angle32::from_turns(2_147_483_648.25), Angle32::QUARTER_TURN);
    assert_eq!(Angle32::from_turns(1e15 + 0.5), Angle32::HALF_TURN);
    assert_eq!(Angle16::from_turns(1e15 + 0.5), Angle16::HALF_TURN);
    assert_eq!(Angle8::from_turns(-1e12 - 0.25), Angle8::THREE_QUARTER_TURN);

    // Every width, every quarter, far past where the old scaling gave out.
    for turns in [0.0_f64, 0.25, 0.5, 0.75] {
        for whole in [0.0_f64, 1.0, 1e6, 2.0_f64.powi(31), 2.0_f64.powi(48)] {
            let expected = Angle32::from_turns(turns);
            assert_eq!(
                Angle32::from_turns(whole + turns),
                expected,
                "{whole} + {turns} turns"
            );
            assert_eq!(
                Angle32::from_turns(-whole + turns),
                expected,
                "-{whole} + {turns} turns"
            );
        }
    }

    // The degree spelling inherits it: a million turns plus ninety degrees.
    assert_eq!(Angle16::from_degrees(360_000_090.0), Angle16::QUARTER_TURN);

    // A circle has no bound to saturate against, so a non-finite angle is zero
    // rather than an arbitrary corner of the range.
    assert_eq!(Angle16::from_turns(f64::INFINITY), Angle16::ZERO);
    assert_eq!(Angle16::from_turns(f64::NEG_INFINITY), Angle16::ZERO);
    assert_eq!(Angle32::from_turns(f64::INFINITY), Angle32::ZERO);
    assert_eq!(Angle16::from_turns(f64::NAN), Angle16::ZERO);
}

#[test]
fn the_angle_checked_conversion_rejects_only_what_needed_wrapping() {
    // Anything that lands on a bit pattern of the same turn converts.
    assert_eq!(Angle8::checked_from_f64(0.0), Some(Angle8::ZERO));
    assert_eq!(
        Angle8::checked_from_f64(255.4 / 256.0),
        Some(Angle8::from_bits(255))
    );
    assert_eq!(
        Angle16::checked_from_f64(0.9999),
        Some(Angle16::from_bits(65_529))
    );

    // A full turn is the next turn's zero, so it is rejected -- and so is
    // anything inside `0.0 .. 1.0` that rounds up onto it. An Angle8 step is
    // 1/256, so 0.999 rounds to 256, which *is* zero.
    assert_eq!(Angle8::checked_from_f64(1.0), None);
    assert_eq!(Angle8::checked_from_f64(0.999), None);
    assert_eq!(
        Angle8::from_turns(0.999),
        Angle8::ZERO,
        "and it wraps to zero"
    );
    assert_eq!(Angle16::checked_from_f64(1.0), None);

    // Below zero the same half-step tolerance applies: an Angle8 step is
    // 1/256, so anything within 1/512 of zero still lands on zero and converts,
    // and anything past that needed wrapping.
    assert_eq!(Angle8::checked_from_f64(-0.001), Some(Angle8::ZERO));
    assert_eq!(Angle8::checked_from_f64(-0.01), None);
    assert_eq!(Angle8::from_turns(-0.01), Angle8::from_bits(253));

    // The finer type accepts what the coarser one cannot, at the same input.
    assert!(Angle16::checked_from_f64(0.999).is_some());
    assert!(Angle32::checked_from_f64(0.999).is_some());
}

#[test]
fn signed_angle_readings_cover_the_negative_half() {
    assert_eq!(Angle16::ZERO.to_signed_turns(), 0.0);
    assert_eq!(Angle16::QUARTER_TURN.to_signed_turns(), 0.25);
    assert_eq!(Angle16::HALF_TURN.to_signed_turns(), -0.5);
    assert_eq!(Angle16::THREE_QUARTER_TURN.to_signed_turns(), -0.25);
    assert!(
        (Angle16::THREE_QUARTER_TURN.to_signed_radians() + core::f64::consts::FRAC_PI_2).abs()
            < 1e-9
    );

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

#[test]
fn the_new_scalars_have_the_documented_range_and_resolution() {
    use corvid_fixed::{I2F30, I16F16, I48F16};

    assert_eq!(I16F16::FRAC_BITS, 16);
    assert_eq!(I48F16::FRAC_BITS, 16);
    assert_eq!(I2F30::FRAC_BITS, 30);

    assert_eq!(I16F16::ONE.to_bits(), 65_536);
    assert_eq!(I48F16::ONE.to_bits(), 65_536);
    assert_eq!(I2F30::ONE.to_bits(), 1 << 30);

    // 1.0 is exactly representable in I2F30, which is why the identity basis
    // is exact.
    assert_eq!(I2F30::ONE.to_f64(), 1.0);
    assert!(I2F30::MAX.to_f64() < 2.0);

    assert_eq!(I16F16::MAX.to_f64(), 32_768.0 - 1.0 / 65_536.0);
    assert_eq!(I16F16::DELTA.to_f64(), 1.0 / 65_536.0);
    assert_eq!(I48F16::DELTA.to_f64(), 1.0 / 65_536.0);

    // I48F16 spans past the Kuiper belt.
    assert!(I48F16::MAX.to_f64() > 1.4e14);
}

#[test]
fn i48f16_is_the_one_type_whose_to_f64_is_lossy() {
    use corvid_fixed::I48F16;

    // 63 magnitude bits exceed f64's 53-bit mantissa, so the round trip is not
    // the identity at the top of the range. Every other type in the family
    // round-trips exactly.
    let wide = I48F16::from_bits((1 << 60) + 1);
    assert_ne!(I48F16::from_f64(wide.to_f64()), wide);
    assert_eq!(I48F16::from_f64(wide.to_f64()).to_bits(), 1 << 60);

    // Anything inside 53 bits still round-trips exactly.
    let ordinary = I48F16::from_f64(6.371e6);
    assert_eq!(I48F16::from_f64(ordinary.to_f64()), ordinary);
}
