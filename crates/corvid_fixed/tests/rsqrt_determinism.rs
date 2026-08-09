//! The golden tables for both reciprocal square roots.
//!
//! `rsqrt` and `rsqrt_fast` are the newest arithmetic here and the widest, so
//! they are pinned twice over: against a table of recorded bits, and against
//! themselves run after run and between the const interpreter and the CPU.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    reason = "the golden tables are raw bit patterns, converted as such"
)]
mod common;

use std::hint::black_box;

use common::Rng;
use corvid_fixed::{I2F30, I16F16, I48F16};
// --- the wider scalars and rsqrt -------------------------------------------

/// Input bits, then the `I2F30::rsqrt` bits they produce.
///
/// `I2F30` is the type the rotation decoders normalize through, so this is the
/// table that pins down every packed rotation in the workspace.
const GOLDEN_RSQRT_I2F30: &[(i32, i32)] = &[
    // 1.0 -> 1.0, exactly.
    (1 << 30, 1 << 30),
    // 0.25 -> 2.0, which is one step past MAX and therefore saturates.
    (1 << 28, i32::MAX),
    // 0.5 -> sqrt(2).
    (1 << 29, 1_518_500_250),
    // 0.75 -> 2/sqrt(3), the axis-aligned case a normalize hits most often.
    (805_306_368, 1_239_850_262),
    // One last bit past 1.0 rounds back onto 1.0.
    (1_073_741_825, 1_073_741_824),
];

/// Input bits, then the `I48F16::rsqrt` bits they produce.
const GOLDEN_RSQRT_I48F16: &[(i64, i64)] = &[
    // 1.0 -> 1.0, and 4.0 -> 0.5.
    (65_536, 65_536),
    (262_144, 32_768),
    // One last bit past 1.0 rounds back onto 1.0.
    (65_537, 65_536),
    // 2^24 -> 2^-12, at the coarse end of the range.
    (1 << 40, 16),
    // The smallest positive value gives the largest answer this type reaches.
    (1, 16_777_216),
];

#[test]
fn rsqrt_matches_its_golden_tables() {
    for &(input, expected) in GOLDEN_RSQRT_I2F30 {
        assert_eq!(
            I2F30::from_bits(input).rsqrt().to_bits(),
            expected,
            "I2F30::rsqrt({input})"
        );
    }
    for &(input, expected) in GOLDEN_RSQRT_I48F16 {
        assert_eq!(
            I48F16::from_bits(input).rsqrt().to_bits(),
            expected,
            "I48F16::rsqrt({input})"
        );
    }
}

#[test]
fn the_wider_scalars_agree_between_const_and_runtime() {
    const NEAR: I16F16 = I16F16::from_f64(1234.5);
    const WIDE: I48F16 = I48F16::from_f64(6_371_000.0);
    const ENTRY: I2F30 = I2F30::from_f64(0.75);

    const NEAR_SQRT: I16F16 = NEAR.sqrt();
    const NEAR_RSQRT: I16F16 = NEAR.rsqrt();
    const WIDE_SQRT: I48F16 = WIDE.sqrt();
    const WIDE_RSQRT: I48F16 = WIDE.rsqrt();
    const ENTRY_RSQRT: I2F30 = ENTRY.rsqrt();
    const ENTRY_PRODUCT: I2F30 = ENTRY.saturating_mul(ENTRY);
    const WIDE_HYPOT: I48F16 = WIDE.hypot(WIDE);

    assert_eq!(NEAR_SQRT, black_box(NEAR).sqrt());
    assert_eq!(NEAR_RSQRT, black_box(NEAR).rsqrt());
    assert_eq!(WIDE_SQRT, black_box(WIDE).sqrt());
    assert_eq!(WIDE_RSQRT, black_box(WIDE).rsqrt());
    assert_eq!(ENTRY_RSQRT, black_box(ENTRY).rsqrt());
    assert_eq!(
        ENTRY_PRODUCT,
        black_box(ENTRY).saturating_mul(black_box(ENTRY))
    );
    assert_eq!(WIDE_HYPOT, black_box(WIDE).hypot(black_box(WIDE)));
}

#[test]
fn rsqrt_gives_the_same_bits_run_after_run() {
    let checksum = || {
        let mut rng = Rng::new(0x2591_2591);
        let mut acc = 0u64;
        for _ in 0..50_000 {
            let bits = (rng.next_u32() >> 2) as i32 | 1;
            acc = acc
                .wrapping_mul(31)
                .wrapping_add(I2F30::from_bits(bits).rsqrt().to_bits() as u64);
            acc = acc
                .wrapping_mul(31)
                .wrapping_add(I16F16::from_bits(bits).rsqrt().to_bits() as u64);
        }
        acc
    };
    // Pinned, not compared to a second run of itself: an identical rerun
    // proves only that the function is a function. This value is what the
    // sequence must stay at, so a rounding or iteration-count change fails
    // here even though it changes every result alike.
    assert_eq!(
        checksum(),
        2_863_920_574_738_504_790,
        "the rsqrt sequence changed"
    );
}

// --- the approximate reciprocal square root --------------------------------
//
// `rsqrt_fast` is pinned for the same reason `rsqrt` is, and more urgently: an
// approximation has no external definition to check against, so the tables
// below *are* its definition. A changed seed, a dropped Newton step, or a
// different shift would all keep the documented error bound and still move
// every packed rotation in the workspace by a few last bits.

/// Input bits, then the `I2F30::rsqrt_fast` bits they produce.
const GOLDEN_RSQRT_FAST_I2F30: &[(i32, i32)] = &[
    // 1.0 -> 1.0, exactly. A power of two is a fixed point of the whole
    // routine, seed included, so the approximation costs nothing here.
    (1 << 30, 1 << 30),
    // 0.25 -> 2.0, which saturates in this tier as well.
    (1 << 28, i32::MAX),
    // 0.5 -> sqrt(2), 3625 last bits above the correctly rounded 1_518_500_250.
    (1 << 29, 1_518_503_875),
    // 0.75 -> 2/sqrt(3), the case a normalize hits most often: 21_884 last bits
    // below the exact 1_239_850_262, or 1.8e-5 relative.
    (805_306_368, 1_239_828_378),
    // One last bit past 1.0 still rounds back onto 1.0.
    (1_073_741_825, 1_073_741_824),
];

/// Input bits, then the `I16F16::rsqrt_fast` bits they produce.
const GOLDEN_RSQRT_FAST_I16F16: &[(i32, i32)] = &[
    // 1.0 -> 1.0, and 4.0 -> 0.5: both exact.
    (65_536, 65_536),
    (262_144, 32_768),
    // One last bit past 1.0 lands a step below the exact answer, which is the
    // whole of what 15 bits costs at this resolution.
    (65_537, 65_535),
    // 2^8 -> 2^-4, and the smallest positive value -> 2^8, both exact.
    (1 << 24, 4_096),
    (1, 16_777_216),
];

#[test]
fn rsqrt_fast_matches_its_golden_tables() {
    for &(input, expected) in GOLDEN_RSQRT_FAST_I2F30 {
        assert_eq!(
            I2F30::from_bits(input).rsqrt_fast().to_bits(),
            expected,
            "I2F30::rsqrt_fast({input})"
        );
    }
    for &(input, expected) in GOLDEN_RSQRT_FAST_I16F16 {
        assert_eq!(
            I16F16::from_bits(input).rsqrt_fast().to_bits(),
            expected,
            "I16F16::rsqrt_fast({input})"
        );
    }
}

#[test]
fn rsqrt_fast_agrees_between_const_and_runtime() {
    const ENTRY: I2F30 = I2F30::from_f64(0.75);
    const NEAR: I16F16 = I16F16::from_f64(1234.5);

    const ENTRY_FAST: I2F30 = ENTRY.rsqrt_fast();
    const NEAR_FAST: I16F16 = NEAR.rsqrt_fast();

    assert_eq!(ENTRY_FAST, black_box(ENTRY).rsqrt_fast());
    assert_eq!(NEAR_FAST, black_box(NEAR).rsqrt_fast());
}

#[test]
fn rsqrt_fast_gives_the_same_bits_run_after_run() {
    let checksum = || {
        let mut rng = Rng::new(0x2591_2591);
        let mut acc = 0u64;
        for _ in 0..50_000 {
            let bits = (rng.next_u32() >> 2) as i32 | 1;
            acc = acc
                .wrapping_mul(31)
                .wrapping_add(I2F30::from_bits(bits).rsqrt_fast().to_bits() as u64);
            acc = acc
                .wrapping_mul(31)
                .wrapping_add(I16F16::from_bits(bits).rsqrt_fast().to_bits() as u64);
        }
        acc
    };
    // Pinned for the same reason the exact tier's checksum is, and it must not
    // equal that one: if it ever does, `rsqrt_fast` has stopped approximating.
    assert_eq!(
        checksum(),
        12_474_794_991_790_848_565,
        "the rsqrt_fast sequence changed"
    );
    assert_ne!(
        checksum(),
        2_863_920_574_738_504_790,
        "rsqrt_fast now matches rsqrt bit for bit"
    );
}
