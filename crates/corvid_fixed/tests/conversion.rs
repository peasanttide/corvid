//! Float conversion: round-trip fidelity, rounding, saturation, and what a
//! `NaN` or an infinity comes to.
//!
//! The 8- and 16-bit types are checked exhaustively in both directions, which
//! is what makes "round-trips" a statement about every value rather than about
//! the ones a sample happened to visit. The angle units are in
//! `tests/angle_conversion.rs`.

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
mod common;

use common::Rng;
use corvid_fixed::{
    Angle8, Angle16, Angle32, Factor8, Factor16, Factor32, I0F8, I8F8, I16F16, I24F8, I48F16,
    Signed8, Signed16, Signed32,
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
    assert_eq!(
        Signed8::from_f64(Signed8::from_bits(-128).to_f64()),
        Signed8::MIN
    );
    assert_eq!(Signed8::from_bits(-128), Signed8::from_bits(-127));
    assert_eq!(Signed16::from_bits(i16::MIN), Signed16::MIN);
    assert_eq!(Signed32::from_bits(i32::MIN), Signed32::MIN);
}

#[test]
fn min_max_and_clamp_fold_the_denormal() {
    // Comparison routes through the canonical form, and so must its result:
    // `min`/`max`/`clamp` are the one way a denormal could otherwise escape a
    // comparison still wearing its non-canonical bits.
    let denormal = Signed8::from_bits(-128);
    assert!(denormal.is_denormal());
    for result in [
        denormal.clamp(Signed8::MIN, Signed8::MAX),
        denormal.min(Signed8::MIN),
        denormal.max(Signed8::MIN),
        denormal.min(denormal),
    ] {
        assert!(
            !result.is_denormal(),
            "denormal survived as {}",
            result.to_bits()
        );
        assert_eq!(result.to_bits(), -127);
    }

    // The wider widths, through `min` and `max` directly as well as `clamp`.
    let wide = Signed16::from_bits(i16::MIN);
    assert!(wide.is_denormal());
    assert_eq!(wide.clamp(Signed16::MIN, Signed16::MAX).to_bits(), -32_767);
    assert_eq!(wide.min(Signed16::MAX).to_bits(), -32_767);
    assert_eq!(wide.max(Signed16::MIN).to_bits(), -32_767);

    let widest = Signed32::from_bits(i32::MIN);
    assert!(widest.is_denormal());
    assert_eq!(
        widest.clamp(Signed32::MIN, Signed32::MAX).to_bits(),
        -2_147_483_647
    );
    assert_eq!(widest.min(Signed32::MAX).to_bits(), -2_147_483_647);
    assert_eq!(widest.max(Signed32::MIN).to_bits(), -2_147_483_647);
}

#[test]
fn the_32_bit_types_round_trip_through_f64() {
    let mut rng = Rng::new(0xc0ff_ee11);
    let mut samples: Vec<u32> = vec![0, 1, 2, u32::MAX, u32::MAX - 1, 1 << 31, (1 << 31) - 1];
    samples.extend((0..50_000).map(|_| rng.next_u32()));

    for raw in samples {
        let signed = raw as i32;

        let fixed = I24F8::from_bits(signed);
        assert_eq!(
            I24F8::from_f64(fixed.to_f64()),
            fixed,
            "I24F8 lost {signed}"
        );

        let factor = Factor32::from_bits(raw);
        assert_eq!(
            Factor32::from_f64(factor.to_f64()),
            factor,
            "Factor32 lost {raw}"
        );

        let snorm = Signed32::from_bits(signed).canonicalize();
        assert_eq!(
            Signed32::from_f64(snorm.to_f64()),
            snorm,
            "Signed32 lost {signed}"
        );

        let angle = Angle32::from_bits(raw);
        assert_eq!(
            Angle32::from_turns(angle.to_turns()),
            angle,
            "Angle32 lost {raw}"
        );
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
    assert_eq!(I8F8::checked_from_f64(127.996_093_75), Some(I8F8::MAX));
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
    assert_eq!(I8F8::from_f64(0.003_906_25).to_bits(), 1);
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

/// Every integer conversion, at the endpoints that decide whether the pair
/// fits.
///
/// The claim `impl_from_int!` makes is that the conversion is exact for every
/// value of the integer type, which is a fact about the *pair* rather than
/// about either half: it holds only while `$int::MIN` and `$int::MAX` shifted
/// by the fractional bits both still land inside `$repr`. These are the four
/// pairs, each at both ends.
#[test]
fn every_integer_conversion_is_exact_at_both_ends() {
    assert_eq!(I8F8::from(i8::MAX).to_f64(), f64::from(i8::MAX));
    assert_eq!(I8F8::from(i8::MIN).to_f64(), f64::from(i8::MIN));

    assert_eq!(I24F8::from(i16::MAX).to_f64(), f64::from(i16::MAX));
    assert_eq!(I24F8::from(i16::MIN).to_f64(), f64::from(i16::MIN));

    assert_eq!(I16F16::from(i16::MAX).to_f64(), f64::from(i16::MAX));
    assert_eq!(I16F16::from(i16::MIN).to_f64(), f64::from(i16::MIN));

    assert_eq!(I48F16::from(i32::MAX).to_f64(), f64::from(i32::MAX));
    assert_eq!(I48F16::from(i32::MIN).to_f64(), f64::from(i32::MIN));
}

/// The next integer type up would not have fitted, which is why each scalar
/// takes the one it does and no wider.
///
/// Checked as arithmetic rather than as a missing impl, because a
/// `compile_fail` doctest would pass for any reason the code failed to
/// compile, including a typo. What is actually being claimed is that the
/// shifted endpoint leaves the representation.
#[test]
fn the_next_integer_up_would_not_have_fitted() {
    // `I8F8` holds an `i8` in an `i16` at Q8; an `i16` would need 24 bits.
    assert!(i32::from(i16::MAX) << 8 > i32::from(i16::MAX));
    // `I24F8` and `I16F16` hold their integer in an `i32`.
    assert!(i64::from(i32::MAX) << 8 > i64::from(i32::MAX));
    assert!(i64::from(i32::MAX) << 16 > i64::from(i32::MAX));
    // `I48F16` holds an `i32` in an `i64`; an `i64` shifted by 16 is not one.
    assert!(i128::from(i64::MAX) << 16 > i128::from(i64::MAX));
}

/// The signed-normalized types take a whole number of units, saturating.
///
/// Unlike the fixed-point conversions above this one is **lossy**, and that is
/// the whole design: the range is `-1.0 ..= 1.0`, so `-1`, `0` and `1` are the
/// only integers it holds and everything else clamps to the nearer end. What
/// it buys is the bare-number spelling at a call site.
#[test]
fn a_signed_normalized_value_takes_a_whole_number_of_units() {
    assert_eq!(Signed32::from(1), Signed32::MAX);
    assert_eq!(Signed32::from(0), Signed32::ZERO);
    assert_eq!(Signed32::from(-1), Signed32::MIN);

    // Saturating rather than wrapping, at both ends and at the extremes.
    assert_eq!(Signed32::from(2), Signed32::MAX);
    assert_eq!(Signed32::from(i32::MAX), Signed32::MAX);
    assert_eq!(Signed32::from(-2), Signed32::MIN);
    assert_eq!(Signed32::from(i32::MIN), Signed32::MIN);

    // The whole family, not just the widest.
    assert_eq!(Signed8::from(1), Signed8::MAX);
    assert_eq!(Signed8::from(-4), Signed8::MIN);
    assert_eq!(Signed16::from(0), Signed16::ZERO);
}
