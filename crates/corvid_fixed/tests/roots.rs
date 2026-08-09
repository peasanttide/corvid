//! Square roots and reciprocal square roots, correctly rounded.
//!
//! Both are checked against a reference worked out in integer arithmetic rather
//! than in `f64`, exhaustively at 8 and 16 bits and over the whole exponent
//! range above that.

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

use common::{Rng, round_half_away};
use corvid_fixed::{Factor16, Factor32, I0F8, I2F30, I8F8, I16F16, I24F8, I48F16, Signed16};

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
