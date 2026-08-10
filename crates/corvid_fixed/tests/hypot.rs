//! The two hypotenuses, held to an integer reference at every width.
//!
//! The reference is `isqrt` on the exact sum of squares, computed one width
//! wider than the kernel under test and rounded once. That is deliberately the
//! obvious implementation of the operation rather than a second copy of the
//! clever one: what these tests are for is the claim that a Newton estimate
//! plus a bounded correction lands on the same bit pattern the library routine
//! does, over every magnitude either can reach.

#![allow(
    clippy::float_cmp,
    reason = "comparisons are against exactly representable references"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "the sweeps walk one integer type and feed it to a narrower one on purpose, which is the boundary being tested"
)]
mod common;

use common::{I32_EDGES, Rng};
use corvid_fixed::{I0F8, I2F30, I8F8, I16F16, I24F8, I48F16};

/// `round(sqrt(sum))`, the answer every case below is checked against.
const fn reference(sum: u128) -> u128 {
    let root = sum.isqrt();
    if sum - root * root > root {
        root + 1
    } else {
        root
    }
}

/// The reference hypotenuse of two bit patterns, saturated at `limit`.
///
/// The squares are summed unsigned because `i64::MIN` in both legs comes to
/// `2^127`, which is one past what `i128` holds and well inside `u128`.
fn reference_hypot(a: i128, b: i128, limit: i128) -> i128 {
    let (a, b) = (a.unsigned_abs(), b.unsigned_abs());
    (reference(a * a + b * b) as i128).min(limit)
}

macro_rules! check {
    ($type:ident, $a:expr, $b:expr) => {{
        let (a, b) = ($type::from_bits($a), $type::from_bits($b));
        let expected = reference_hypot(
            i128::from($a),
            i128::from($b),
            i128::from($type::MAX.to_bits()),
        );
        assert_eq!(
            i128::from(a.hypot(b).to_bits()),
            expected,
            concat!(stringify!($type), "::hypot({}, {})"),
            $a,
            $b
        );
    }};
}

#[test]
fn hypot_is_exact_over_every_i0f8_pair() {
    for a in i8::MIN..=i8::MAX {
        for b in i8::MIN..=i8::MAX {
            check!(I0F8, a, b);
        }
    }
}

#[test]
fn hypot_is_exact_at_the_boundaries_of_the_32_bit_types() {
    // Every pair of edge patterns, including both `i32::MIN` legs, whose sum of
    // squares is `2^63` -- the largest a 32-bit type can present, and the one
    // input the kernel's normalization has no leading zero to work with.
    for &a in I32_EDGES {
        for &b in I32_EDGES {
            check!(I24F8, a, b);
            check!(I16F16, a, b);
            check!(I2F30, a, b);
            check!(I48F16, i64::from(a), i64::from(b));
        }
    }

    for &a in I32_EDGES {
        for &b in I32_EDGES {
            check!(I48F16, i64::from(a) << 32, i64::from(b) << 32);
            check!(I48F16, i64::from(a) << 31, i64::from(b) << 31);
        }
    }
    check!(I48F16, i64::MIN, i64::MIN);
    check!(I48F16, i64::MAX, i64::MAX);
    check!(I48F16, i64::MIN, 0);
}

#[test]
fn hypot_is_exact_around_every_perfect_square() {
    // A perfect square and its neighbours are where a floor and a
    // round-to-nearest disagree, so they are where an estimate one out shows
    // up. Walking a leg past every power of two covers both at every exponent.
    for shift in 0..31 {
        for offset in -2_i64..=2 {
            let a = (1_i64 << shift) + offset;
            if a <= 0 {
                continue;
            }
            for &b in &[0, 1, a - 1, a, a + 1, 3 * a / 2] {
                check!(I16F16, a as i32, b as i32);
                check!(I24F8, a as i32, b as i32);
                check!(I48F16, a, b);
                check!(I48F16, a << 32, b << 32);
            }
        }
    }
}

#[test]
fn hypot_is_exact_at_every_operand_width() {
    // The kernel's cost is flat but its normalization is not: the shift it
    // takes depends on the leading zeros of the sum, so every operand width is
    // a different path through it.
    let mut rng = Rng::new(0x1618_0339);
    for bits in 1..=31_u32 {
        let mask = ((1_u64 << bits) - 1) as i32;
        for _ in 0..2_000 {
            let a = (rng.next_u32() as i32) & mask;
            let b = (rng.next_u32() as i32) & mask;
            check!(I16F16, a, b);
            check!(I16F16, -a, b);
            check!(I2F30, a, -b);
        }
    }
    for bits in 1..=63_u32 {
        let mask = ((1_u128 << bits) - 1) as i64;
        for _ in 0..2_000 {
            let a = (rng.next_u64() as i64) & mask;
            let b = (rng.next_u64() as i64) & mask;
            check!(I48F16, a, b);
            check!(I48F16, -a, b);
        }
    }
}

#[test]
fn hypot_saturates_rather_than_wrapping() {
    // Two squares that overflow the storage type are summed at full width, so
    // what a caller sees is a clamp rather than a wrap.
    assert_eq!(I24F8::MAX.hypot(I24F8::MAX), I24F8::MAX);
    assert_eq!(I24F8::MIN.hypot(I24F8::MIN), I24F8::MAX);
    assert_eq!(I8F8::MAX.hypot(I8F8::MAX), I8F8::MAX);
    assert_eq!(I16F16::MIN.hypot(I16F16::MIN), I16F16::MAX);
    assert_eq!(I48F16::MIN.hypot(I48F16::MIN), I48F16::MAX);
    assert_eq!(I2F30::MAX.hypot(I2F30::MAX), I2F30::MAX);

    // The result is never negative, so `MIN` is unreachable and the sign of
    // either argument never survives.
    assert_eq!(
        I16F16::from_f64(-3.0).hypot(I16F16::from_f64(-4.0)),
        I16F16::from_f64(5.0)
    );
}

#[test]
fn a_zero_leg_gives_the_other_leg_back_exactly() {
    // `sqrt(a^2)` is `a`, so this is the identity that pins down the rounding:
    // an estimate one out anywhere would show up as a value one out here.
    for bits in i16::MIN..=i16::MAX {
        let value = I8F8::from_bits(bits);
        assert_eq!(
            value.hypot(I8F8::ZERO),
            value.abs(),
            "hypot of {bits} and 0"
        );
        assert_eq!(
            I8F8::ZERO.hypot(value),
            value.abs(),
            "hypot of 0 and {bits}"
        );
    }
    assert_eq!(I16F16::ZERO.hypot(I16F16::ZERO), I16F16::ZERO);
}

#[test]
fn hypot1_is_the_hypotenuse_with_a_leg_of_one() {
    for bits in i16::MIN..=i16::MAX {
        let value = I8F8::from_bits(bits);
        assert_eq!(value.hypot1(), value.hypot(I8F8::ONE), "hypot1 of {bits}");
    }

    let mut rng = Rng::new(0x2653_5897);
    for _ in 0..20_000 {
        let bits = rng.next_u32() as i32;
        assert_eq!(
            I16F16::from_bits(bits).hypot1(),
            I16F16::from_bits(bits).hypot(I16F16::ONE),
            "I16F16::hypot1 of {bits}"
        );
        assert_eq!(
            I2F30::from_bits(bits).hypot1(),
            I2F30::from_bits(bits).hypot(I2F30::ONE),
            "I2F30::hypot1 of {bits}"
        );
        let wide = rng.next_u64() as i64;
        assert_eq!(
            I48F16::from_bits(wide).hypot1(),
            I48F16::from_bits(wide).hypot(I48F16::ONE),
            "I48F16::hypot1 of {wide}"
        );
    }

    // The named cases: the length of (0, 1) is one, and 3-4-5 read as a slope.
    assert_eq!(I16F16::ZERO.hypot1(), I16F16::ONE);
    assert_eq!(I16F16::from_f64(0.75).hypot1(), I16F16::from_f64(1.25));
    assert_eq!(I16F16::from_f64(-0.75).hypot1(), I16F16::from_f64(1.25));

    // `I0F8` holds nothing as large as one, so every result saturates -- which
    // is also why the operation cannot be spelled `self.hypot(Self::ONE)`.
    assert_eq!(I0F8::ZERO.hypot1(), I0F8::MAX);
    assert_eq!(I0F8::MIN.hypot1(), I0F8::MAX);
}

#[test]
fn hypot1_is_exact_against_the_reference() {
    for bits in i8::MIN..=i8::MAX {
        let expected = reference_hypot(i128::from(bits), 256, i128::from(I0F8::MAX.to_bits()));
        assert_eq!(
            i128::from(I0F8::from_bits(bits).hypot1().to_bits()),
            expected
        );
    }

    let mut rng = Rng::new(0x9323_8462);
    for _ in 0..20_000 {
        let bits = rng.next_u32() as i32;
        for (name, got, one, limit) in [
            (
                "I16F16",
                I16F16::from_bits(bits).hypot1().to_bits(),
                1 << 16,
                I16F16::MAX.to_bits(),
            ),
            (
                "I2F30",
                I2F30::from_bits(bits).hypot1().to_bits(),
                1 << 30,
                I2F30::MAX.to_bits(),
            ),
        ] {
            let expected = reference_hypot(i128::from(bits), one, i128::from(limit));
            assert_eq!(i128::from(got), expected, "{name}::hypot1 of {bits}");
        }
    }
}

#[test]
fn both_hypotenuses_are_available_in_const_context() {
    const LEG: I16F16 = I16F16::from_f64(3.0).hypot(I16F16::from_f64(4.0));
    const SLOPE: I16F16 = I16F16::from_f64(0.75).hypot1();
    const WIDE: I48F16 = I48F16::from_f64(1e10).hypot(I48F16::from_f64(1e10));

    assert_eq!(LEG.to_f64(), 5.0);
    assert_eq!(SLOPE.to_f64(), 1.25);
    assert_eq!(
        WIDE,
        I48F16::from_bits(reference_hypot(
            i128::from(I48F16::from_f64(1e10).to_bits()),
            i128::from(I48F16::from_f64(1e10).to_bits()),
            i128::from(I48F16::MAX.to_bits()),
        ) as i64)
    );
}
