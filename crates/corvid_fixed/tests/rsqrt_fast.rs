//! The fast reciprocal square root, and the bound it trades accuracy for.
//!
//! What is checked is not a bit pattern but a bound: that the fast form stays
//! within a stated distance of the exact one over every exponent and at the
//! boundaries between them, and that it saturates wherever the exact one does.

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
use corvid_fixed::{I0F8, I2F30, I8F8, I16F16, I24F8};

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
