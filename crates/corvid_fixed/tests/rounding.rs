//! The roundings, the reciprocal, and the two composed operations.
//!
//! Each is held to a reference computed in `f64` and rounded once, which is
//! what `mul_add` and `hypot` promise and what a naive composition of the
//! primitive operations would fail.

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
use corvid_fixed::{
    Angle16, Factor16, Factor32, I0F8, I8F8, I24F8, I48F16, Pitch16, Signed8, Signed16,
};

#[test]
fn rounding_matches_the_float_reference_exhaustively() {
    for bits in i16::MIN..=i16::MAX {
        let value = I8F8::from_bits(bits);
        let exact = value.to_f64();

        // I8F8 cannot hold 128.0, so anything that rounds up out of range
        // saturates; compare against the clamped reference.
        let low = I8F8::MIN.to_f64();
        let high = I8F8::MAX.to_f64();
        let floor = exact.floor().max(low);
        let ceil = exact.ceil().min(high);
        let round = round_half_away(exact).clamp(low, high);
        let trunc = exact.trunc();

        assert_eq!(value.floor().to_f64(), floor, "floor of {exact}");
        assert_eq!(value.ceil().to_f64(), ceil, "ceil of {exact}");
        assert_eq!(value.round().to_f64(), round, "round of {exact}");
        assert_eq!(value.trunc().to_f64(), trunc, "trunc of {exact}");
        assert_eq!(value.fract().to_f64(), exact - trunc, "fract of {exact}");

        // The defining identity, which also pins down the sign of fract.
        assert_eq!(value.trunc().to_f64() + value.fract().to_f64(), exact);
    }
}

#[test]
fn rounding_lands_on_integers_and_leaves_them_alone() {
    for whole in -100_i32..=100 {
        let value = I24F8::from_f64(f64::from(whole));
        assert_eq!(value.floor(), value, "floor moved {whole}");
        assert_eq!(value.ceil(), value, "ceil moved {whole}");
        assert_eq!(value.round(), value, "round moved {whole}");
        assert_eq!(value.trunc(), value, "trunc moved {whole}");
        assert_eq!(value.fract(), I24F8::ZERO, "fract of {whole} was not zero");
    }

    // Halfway cases go away from zero, like f64::round.
    assert_eq!(I24F8::from_f64(0.5).round().to_f64(), 1.0);
    assert_eq!(I24F8::from_f64(-0.5).round().to_f64(), -1.0);
    assert_eq!(I24F8::from_f64(1.5).round().to_f64(), 2.0);
    assert_eq!(I24F8::from_f64(-1.5).round().to_f64(), -2.0);
    assert_eq!(I24F8::from_f64(0.49).round(), I24F8::ZERO);

    // Floor and ceil differ from trunc on the negative side.
    assert_eq!(I24F8::from_f64(-2.5).floor().to_f64(), -3.0);
    assert_eq!(I24F8::from_f64(-2.5).ceil().to_f64(), -2.0);
    assert_eq!(I24F8::from_f64(-2.5).trunc().to_f64(), -2.0);
}

#[test]
fn rounding_saturates_instead_of_leaving_the_range() {
    assert_eq!(I8F8::MAX.ceil(), I8F8::MAX);
    assert_eq!(I8F8::MAX.round(), I8F8::MAX);
    assert_eq!(I8F8::MAX.floor().to_f64(), 127.0);
    assert_eq!(I8F8::MIN.floor(), I8F8::MIN);
    assert_eq!(I8F8::MIN.ceil(), I8F8::MIN);

    // Every I0F8 value is under 0.5 in magnitude, so trunc is always zero and
    // fract is always the whole value.
    for bits in i8::MIN..=i8::MAX {
        let value = I0F8::from_bits(bits);
        assert_eq!(value.trunc(), I0F8::ZERO, "trunc of {bits}");
        assert_eq!(value.fract(), value, "fract of {bits}");
    }
    assert_eq!(
        I0F8::from_f64(0.25).ceil(),
        I0F8::MAX,
        "ceil to 1.0 must saturate"
    );
    assert_eq!(
        I0F8::from_f64(-0.25).floor(),
        I0F8::MIN,
        "floor to -1.0 must saturate"
    );
}

#[test]
fn the_reciprocal_is_correct_and_total() {
    for bits in i16::MIN..=i16::MAX {
        let value = I8F8::from_bits(bits);
        if bits == 0 {
            assert_eq!(value.recip(), I8F8::MAX);
            assert_eq!(value.checked_recip(), None);
            continue;
        }
        let exact = round_half_away(65_536.0 / f64::from(bits));
        let clamped = exact.clamp(f64::from(i16::MIN), f64::from(i16::MAX));
        assert_eq!(
            f64::from(value.recip().to_bits()),
            clamped,
            "recip of {bits}"
        );
        assert_eq!(
            value.checked_recip().is_none(),
            exact != clamped,
            "recip of {bits}"
        );
    }

    assert_eq!(I24F8::ONE.recip(), I24F8::ONE);
    assert_eq!(I24F8::from_f64(2.0).recip().to_f64(), 0.5);
    assert_eq!(I24F8::from_f64(-4.0).recip().to_f64(), -0.25);
    assert_eq!(I24F8::from_f64(0.25).recip().to_f64(), 4.0);
    // Everything an I0F8 can hold has a reciprocal of at least 2.
    assert_eq!(I0F8::from_f64(0.25).recip(), I0F8::MAX);
    assert_eq!(I0F8::from_f64(-0.25).checked_recip(), None);
}

#[test]
fn mul_add_rounds_only_once() {
    // Multiplying and then adding rounds twice, so it can differ by a step. The
    // fused form is the one that matches the true value.
    let small = I24F8::from_bits(3);
    let one_step = I24F8::from_bits(1);
    // 3/256 * 3/256 is 9/65536, well under half a step, so it must vanish.
    assert_eq!(small.mul_add(small, one_step), one_step);

    let base = I24F8::from_f64(1.5);
    let scale = I24F8::from_f64(2.0);
    let offset = I24F8::from_f64(-0.25);
    assert_eq!(base.mul_add(scale, offset).to_f64(), 2.75);
    assert_eq!(
        base.mul_add(scale, offset),
        base.saturating_mul(scale).saturating_add(offset)
    );

    // Saturates like everything else.
    assert_eq!(I24F8::MAX.mul_add(I24F8::MAX, I24F8::ZERO), I24F8::MAX);
    assert_eq!(I24F8::MAX.mul_add(I24F8::MIN, I24F8::ZERO), I24F8::MIN);
    assert_eq!(I24F8::ZERO.mul_add(I24F8::MAX, I24F8::ONE), I24F8::ONE);

    // Against a reference, across a spread of magnitudes.
    let mut rng = Rng::new(0x3141_5926);
    for _ in 0..20_000 {
        let a = I8F8::from_bits(rng.next_u32() as i16);
        let b = I8F8::from_bits(rng.next_u32() as i16);
        let c = I8F8::from_bits(rng.next_u32() as i16);
        let exact = round_half_away(
            f64::from(a.to_bits()) * f64::from(b.to_bits()) / 256.0 + f64::from(c.to_bits()),
        );
        let expected = exact.clamp(f64::from(i16::MIN), f64::from(i16::MAX));
        assert_eq!(f64::from(a.mul_add(b, c).to_bits()), expected);
    }
}

#[test]
fn hypot_is_correct_and_never_overflows() {
    assert_eq!(
        I24F8::from_f64(3.0).hypot(I24F8::from_f64(4.0)).to_f64(),
        5.0
    );
    assert_eq!(
        I24F8::from_f64(-3.0).hypot(I24F8::from_f64(4.0)).to_f64(),
        5.0
    );
    assert_eq!(
        I24F8::from_f64(-3.0).hypot(I24F8::from_f64(-4.0)).to_f64(),
        5.0
    );
    assert_eq!(I24F8::ZERO.hypot(I24F8::ZERO), I24F8::ZERO);
    assert_eq!(I24F8::from_f64(5.0).hypot(I24F8::ZERO).to_f64(), 5.0);

    // Two large squares would overflow the storage type. The sum is formed at
    // double width, so the result merely saturates.
    assert_eq!(I24F8::MAX.hypot(I24F8::MAX), I24F8::MAX);
    assert_eq!(I24F8::MIN.hypot(I24F8::MIN), I24F8::MAX);
    assert_eq!(I8F8::MAX.hypot(I8F8::MAX), I8F8::MAX);

    let mut rng = Rng::new(0x2718_2818);
    for _ in 0..20_000 {
        let a = I8F8::from_bits((rng.next_u32() as i16) / 2);
        let b = I8F8::from_bits((rng.next_u32() as i16) / 2);
        let exact = round_half_away(f64::from(a.to_bits()).hypot(f64::from(b.to_bits())));
        let expected = exact.min(f64::from(i16::MAX));
        assert_eq!(
            f64::from(a.hypot(b).to_bits()),
            expected,
            "hypot of {a} and {b}"
        );
    }
}

#[test]
fn the_float_style_functions_are_available_in_const_context() {
    const VALUE: I24F8 = I24F8::from_f64(-2.75);
    const FLOOR: I24F8 = VALUE.floor();
    const CEIL: I24F8 = VALUE.ceil();
    const ROUND: I24F8 = VALUE.round();
    const TRUNC: I24F8 = VALUE.trunc();
    const FRACT: I24F8 = VALUE.fract();
    const RECIP: I24F8 = I24F8::from_f64(4.0).recip();
    const FUSED: I24F8 = VALUE.mul_add(I24F8::from_f64(2.0), I24F8::ONE);
    const LEG: I24F8 = I24F8::from_f64(3.0).hypot(I24F8::from_f64(4.0));

    assert_eq!(FLOOR.to_f64(), -3.0);
    assert_eq!(CEIL.to_f64(), -2.0);
    assert_eq!(ROUND.to_f64(), -3.0);
    assert_eq!(TRUNC.to_f64(), -2.0);
    assert_eq!(FRACT.to_f64(), -0.75);
    assert_eq!(RECIP.to_f64(), 0.25);
    assert_eq!(FUSED.to_f64(), -4.5);
    assert_eq!(LEG.to_f64(), 5.0);
}

#[test]
fn arithmetic_is_available_in_const_context() {
    const A: I24F8 = I24F8::from_f64(1.5);
    const B: I24F8 = I24F8::from_f64(-0.25);
    const SUM: I24F8 = A.saturating_add(B);
    const PRODUCT: I24F8 = A.saturating_mul(B);
    const QUOTIENT: I24F8 = A.saturating_div(B);
    const ROOT: I24F8 = I24F8::from_f64(2.25).sqrt();
    const MIDPOINT: I24F8 = A.lerp(B, Factor32::from_f64(0.5));
    const CHECKED: Option<I24F8> = I24F8::MAX.checked_add(I24F8::DELTA);
    const OVERFLOWED: bool = I24F8::MAX.overflowing_add(I24F8::DELTA).1;

    assert_eq!(SUM.to_f64(), 1.25);
    assert_eq!(PRODUCT.to_f64(), -0.375);
    assert_eq!(QUOTIENT.to_f64(), -6.0);
    assert_eq!(ROOT.to_f64(), 1.5);
    assert_eq!(MIDPOINT.to_f64(), 0.625);
    assert_eq!(CHECKED, None);
    assert_eq!(OVERFLOWED, I24F8::MAX.overflowing_add(I24F8::DELTA).1);

    const FACTOR: Factor16 = Factor16::MAX.mul(Factor16::from_f64(0.5));
    const SNORM: Signed16 = Signed16::MIN.neg();
    const ARC: Angle16 = Angle16::HALF_TURN.wrapping_add(Angle16::QUARTER_TURN);
    assert_eq!(FACTOR, Factor16::from_f64(0.5));
    assert_eq!(SNORM, Signed16::MAX);
    assert_eq!(ARC, Angle16::THREE_QUARTER_TURN);
}

#[test]
fn comparison_is_available_in_const_context() {
    // `min`/`max`/`clamp` canonicalize their result, which for the signed and
    // pitch families is real work rather than a move -- so it has to survive
    // const evaluation, and the const and runtime paths have to agree on the
    // bits and not merely on the value.
    const LOW: I24F8 = I24F8::from_f64(-1.0);
    const HIGH: I24F8 = I24F8::from_f64(1.0);
    const CLAMPED: I24F8 = I24F8::from_f64(5.0).clamp(LOW, HIGH);
    const LESSER: Factor16 = Factor16::MAX.min(Factor16::from_bits(10));
    const GREATER: Angle16 = Angle16::MAX.max(Angle16::ZERO);
    const FOLDED: Signed8 = Signed8::from_bits(i8::MIN).clamp(Signed8::MIN, Signed8::MAX);
    const NARROWED: Pitch16 = Pitch16::from_bits(20_000).min(Pitch16::MAX);

    assert_eq!(CLAMPED, HIGH);
    assert_eq!(LESSER.to_bits(), 10);
    assert_eq!(GREATER, Angle16::MAX);
    assert_eq!(
        FOLDED.to_bits(),
        -127,
        "the denormal survived const folding"
    );
    assert_eq!(NARROWED.to_bits(), 16_384);

    // `black_box` keeps the compiler from folding these back into the constants
    // above, so this really compares the two evaluators against each other.
    let denormal = black_box(Signed8::from_bits(i8::MIN));
    let out_of_range = black_box(Pitch16::from_bits(20_000));
    assert_eq!(
        FOLDED.to_bits(),
        denormal.clamp(Signed8::MIN, Signed8::MAX).to_bits()
    );
    assert_eq!(NARROWED.to_bits(), out_of_range.min(Pitch16::MAX).to_bits());
}

/// `split_floor` reconstructs its input and keeps the remainder under one, for
/// every bit pattern of a whole type.
///
/// The reconstruction is the property the operation exists for -- a caller
/// subtracts the whole part in integers and converts only the remainder, so a
/// pair that does not add up is a position quietly moved.
#[test]
fn splitting_at_the_floor_reconstructs_exhaustively() {
    for bits in i16::MIN..=i16::MAX {
        let value = I8F8::from_bits(bits);
        let (whole, remainder) = value.split_floor();

        assert_eq!(
            f64::from(whole) + remainder.to_f64(),
            value.to_f64(),
            "{value:?} split as ({whole}, {remainder:?})"
        );
        assert!(
            remainder >= I8F8::ZERO && remainder < I8F8::ONE,
            "remainder {remainder:?} of {value:?} is not in [0, 1)"
        );
        assert_eq!(
            f64::from(whole),
            value.floor().to_f64(),
            "the whole part is not the floor of {value:?}"
        );
    }
}

/// Below zero it floors where `trunc`/`fract` truncate, which is the whole
/// reason it is a separate operation.
#[test]
fn splitting_at_the_floor_differs_from_truncation_below_zero() {
    let quarter_below = I8F8::from_f64(-0.25);

    assert_eq!(quarter_below.trunc().to_f64(), 0.0);
    assert_eq!(quarter_below.fract().to_f64(), -0.25);

    let (whole, remainder) = quarter_below.split_floor();
    assert_eq!(whole, -1);
    assert_eq!(remainder.to_f64(), 0.75);

    // Above zero the two agree, so nothing is gained by using both.
    let quarter_above = I8F8::from_f64(0.25);
    assert_eq!(quarter_above.split_floor(), (0, quarter_above.fract()));
}

/// A whole part past an `i32` saturates and the remainder absorbs the rest, so
/// the pair still sums to the input rather than losing the overflow.
#[test]
fn splitting_saturates_the_whole_part_without_losing_it() {
    // Ten million kilometres: past an i32 of metres, well inside an I48F16.
    let far = I48F16::from_f64(1e10);
    let (whole, remainder) = far.split_floor();

    assert_eq!(whole, i32::MAX);
    assert_eq!(
        f64::from(whole) + remainder.to_f64(),
        far.to_f64(),
        "the saturated whole part and its remainder no longer sum to the input"
    );

    let below = I48F16::from_f64(-1e10);
    let (whole, remainder) = below.split_floor();
    assert_eq!(whole, i32::MIN);
    assert_eq!(f64::from(whole) + remainder.to_f64(), below.to_f64());
}

/// It is `const`, which is what lets a projection be one.
#[test]
fn splitting_at_the_floor_is_const() {
    const SPLIT: (i32, I24F8) = I24F8::from_f64(-2.5).split_floor();

    assert_eq!(SPLIT.0, -3);
    assert_eq!(SPLIT.1.to_f64(), 0.5);
}
