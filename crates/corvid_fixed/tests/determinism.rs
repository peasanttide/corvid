//! Locks the results down: the golden tables for the trigonometry and the
//! arithmetic, and the agreement between the const interpreter and the CPU.
//!
//! A number in one of these tables is a promise to every peer that ever
//! recorded a replay. Regenerating one to make a red test go green is never the
//! right move; the red test is the notification. The reciprocal square roots
//! have their own file.

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
use corvid_fixed::{Angle16, Angle32, Factor16, Factor32, I24F8, Signed16, Signed32};
const GOLDEN_SIN16: &[(u16, i16)] = &[
    (0, 0),
    (1, 3),
    (1000, 3137),
    (8192, 23170),
    (16384, 32767),
    (20000, 30818),
    (32768, 0),
    (40000, -20942),
    (49152, -32767),
    (60000, -16586),
    (65535, -3),
    (12345, 30341),
];

const GOLDEN_COS32: &[(u32, i32)] = &[
    (0, 2_147_483_647),
    (1, 2_147_483_647),
    (1_000_000_007, 231_217_667),
    (2_147_483_648, -2_147_483_647),
    (3_000_000_000, -682_931_371),
    (4_294_967_295, 2_147_483_647),
];

const GOLDEN_TAN: &[(u16, i32)] = &[
    (1000, 25),
    (8192, 256),
    (16000, 6950),
    (30000, -70),
    (45000, 609),
    (60000, -150),
];

const GOLDEN_ATAN2: &[(i64, i64, u16)] = &[
    (1, 3, 3356),
    (-7, 2, 52055),
    (100, -100, 24576),
    (0, -1, 32768),
    (1_000_000, 3, 16384),
    (-3, -4, 39480),
    (i64::MAX, 1, 16384),
    (5, 12, 4118),
];

const GOLDEN_MUL: &[(i32, i32, i32)] = &[
    (384, -64, -96),
    (1, 1, 0),
    (-1, 1, 0),
    (100_000, 300, 117_188),
    (i32::MAX, 2, 16_777_216),
    (12345, 6789, 327_384),
];

const GOLDEN_SQRT: &[(i32, i32)] = &[
    (0, 0),
    (1, 16),
    (256, 256),
    (512, 362),
    (1000, 506),
    (i32::MAX, 741_455),
];

const GOLDEN_FACTOR_MUL: &[(u16, u16, u16)] = &[
    (1, 1, 0),
    (32768, 32768, 16384),
    (65535, 12345, 12345),
    (60000, 60000, 54932),
    (7, 9, 0),
];

const GOLDEN_LERP: &[(i32, i32, u32, i32)] = &[
    (0, 1000, 1_000_000_000, 233),
    (-500, 500, 2_147_483_648, 0),
    (7, 9, 1, 7),
    (i32::MIN, i32::MAX, 3_000_000_000, 852_516_352),
];

const GOLDEN_SNORM_DIV: &[(i16, i16, i16)] = &[
    (1, 2, 16384),
    (-32767, 3, -32767),
    (100, -7, -32767),
    (32767, 32767, 32767),
    (5, 1, 32767),
];

#[test]
fn sine_matches_the_golden_table() {
    for &(bits, expected) in GOLDEN_SIN16 {
        assert_eq!(
            Angle16::from_bits(bits).sin().to_bits(),
            expected,
            "sin at {bits}"
        );
    }
}

#[test]
fn cosine_matches_the_golden_table() {
    for &(bits, expected) in GOLDEN_COS32 {
        assert_eq!(
            Angle32::from_bits(bits).cos().to_bits(),
            expected,
            "cos at {bits}"
        );
    }
}

#[test]
fn tangent_matches_the_golden_table() {
    for &(bits, expected) in GOLDEN_TAN {
        assert_eq!(
            Angle16::from_bits(bits).tan().to_bits(),
            expected,
            "tan at {bits}"
        );
    }
}

#[test]
fn atan2_matches_the_golden_table() {
    for &(y, x, expected) in GOLDEN_ATAN2 {
        assert_eq!(Angle16::atan2(y, x).to_bits(), expected, "atan2({y}, {x})");
    }
}

#[test]
fn multiplication_matches_the_golden_table() {
    for &(a, b, expected) in GOLDEN_MUL {
        let product = I24F8::from_bits(a).saturating_mul(I24F8::from_bits(b));
        assert_eq!(product.to_bits(), expected, "{a} * {b}");
    }
}

#[test]
fn square_root_matches_the_golden_table() {
    for &(a, expected) in GOLDEN_SQRT {
        assert_eq!(
            I24F8::from_bits(a).sqrt().to_bits(),
            expected,
            "sqrt of {a}"
        );
    }
}

#[test]
fn factor_multiplication_matches_the_golden_table() {
    for &(a, b, expected) in GOLDEN_FACTOR_MUL {
        let product = Factor16::from_bits(a).mul(Factor16::from_bits(b));
        assert_eq!(product.to_bits(), expected, "{a} * {b}");
    }
}

#[test]
fn interpolation_matches_the_golden_table() {
    for &(a, b, t, expected) in GOLDEN_LERP {
        let mixed = I24F8::from_bits(a).lerp(I24F8::from_bits(b), Factor32::from_bits(t));
        assert_eq!(mixed.to_bits(), expected, "lerp({a}, {b}, {t})");
    }
}

#[test]
fn signed_division_matches_the_golden_table() {
    for &(a, b, expected) in GOLDEN_SNORM_DIV {
        let quotient = Signed16::from_bits(a).saturating_div(Signed16::from_bits(b));
        assert_eq!(quotient.to_bits(), expected, "{a} / {b}");
    }
}

// Compile-time results for the const-versus-runtime comparison below. Declared
// at file scope so the const interpreter is the only thing that ever evaluates
// them.
const COMPILED_SIN: Signed16 = Angle16::from_bits(12345).sin();
const COMPILED_COS: Signed16 = Angle16::from_bits(12345).cos();
const COMPILED_SIN_FAST: Signed16 = Angle16::from_bits(12345).sin_fast();
const COMPILED_SIN32: Signed32 = Angle32::from_bits(3_000_000_007).sin();
const COMPILED_TAN: I24F8 = Angle16::from_bits(16000).tan();
const COMPILED_ATAN2: Angle16 = Angle16::atan2(-31, 17);
const COMPILED_ATAN2_FAST: Angle16 = Angle16::atan2_fast(-31, 17);
const COMPILED_FROM_DEGREES: Angle32 = Angle32::from_degrees(123.456);
const COMPILED_PRODUCT: I24F8 = I24F8::from_f64(12.5).saturating_mul(I24F8::from_f64(-3.25));
const COMPILED_QUOTIENT: I24F8 = I24F8::from_f64(12.5).saturating_div(I24F8::from_f64(-3.25));
const COMPILED_ROOT: I24F8 = I24F8::from_f64(1234.5).sqrt();
const COMPILED_MIXED: I24F8 =
    I24F8::from_f64(-7.5).lerp(I24F8::from_f64(11.25), Factor32::from_f64(0.3));
const COMPILED_CONVERTED: I24F8 = I24F8::from_f64(-98765.4321);

#[test]
fn the_const_interpreter_and_the_cpu_agree() {
    // Every right-hand side is computed at run time from values the optimizer
    // cannot fold, since they arrive through a black box.
    let angle = black_box(Angle16::from_bits(12345));
    assert_eq!(COMPILED_SIN, angle.sin());
    assert_eq!(COMPILED_COS, angle.cos());
    assert_eq!(COMPILED_SIN_FAST, angle.sin_fast());
    assert_eq!(COMPILED_TAN, black_box(Angle16::from_bits(16000)).tan());
    assert_eq!(
        COMPILED_SIN32,
        black_box(Angle32::from_bits(3_000_000_007)).sin()
    );

    let (y, x) = black_box((-31_i64, 17_i64));
    assert_eq!(COMPILED_ATAN2, Angle16::atan2(y, x));
    let (fast_y, fast_x) = black_box((-31_i32, 17_i32));
    assert_eq!(COMPILED_ATAN2_FAST, Angle16::atan2_fast(fast_y, fast_x));
    assert_eq!(
        COMPILED_FROM_DEGREES,
        Angle32::from_degrees(black_box(123.456_f64))
    );

    let a = black_box(I24F8::from_f64(12.5));
    let b = black_box(I24F8::from_f64(-3.25));
    assert_eq!(COMPILED_PRODUCT, a.saturating_mul(b));
    assert_eq!(COMPILED_QUOTIENT, a.saturating_div(b));
    assert_eq!(COMPILED_ROOT, black_box(I24F8::from_f64(1234.5)).sqrt());
    assert_eq!(
        COMPILED_MIXED,
        black_box(I24F8::from_f64(-7.5)).lerp(
            black_box(I24F8::from_f64(11.25)),
            black_box(Factor32::from_f64(0.3))
        )
    );
    assert_eq!(COMPILED_CONVERTED, I24F8::from_f64(black_box(-98765.4321)));
}

#[test]
fn a_long_mixed_sequence_reproduces_exactly() {
    // Stands in for a simulation tick: a deterministic stream of operations
    // whose final state is a single number. Any change anywhere in the crate
    // moves this, which is the point.
    fn run() -> (i32, u16, u16, i16) {
        let mut rng = Rng::new(0xd0d0_face);
        let mut position = I24F8::ZERO;
        let mut heading = Angle16::ZERO;
        let mut throttle = Factor16::from_f64(0.5);
        let mut lean = Signed16::ZERO;

        for _ in 0..10_000 {
            let noise = rng.next_u32();
            heading += Angle16::from_bits(noise as u16);
            let (sin, cos) = heading.sin_cos();

            position = position.saturating_add(I24F8::from_bits(i32::from(cos.to_bits()) / 128));
            position = position.lerp(I24F8::from_f64(100.0), Factor32::from_bits(noise / 64));

            throttle = throttle
                .mul(Factor16::from_bits(60_000))
                .saturating_add(Factor16::DELTA);
            lean = lean
                .mul(sin)
                .saturating_sub(Signed16::from_bits(sin.to_bits() / 4));

            if noise.is_multiple_of(7) {
                heading = Angle16::atan2(i64::from(sin.to_bits()), i64::from(cos.to_bits()));
                position = position.sqrt();
            }
        }

        (
            position.to_bits(),
            heading.to_bits(),
            throttle.to_bits(),
            lean.to_bits(),
        )
    }

    let first = run();
    assert_eq!(first, run(), "the same sequence gave two different answers");
    assert_eq!(first, (1906, 18105, 17, -7195), "the sequence changed");
}
