//! The edges, which are the whole reason this crate exists.
//!
//! Every one of these was previously covered -- a little differently -- by
//! whichever crate had written the helper out for itself. Pinning them once is
//! what makes the fourteen copies safe to delete.

use corvid_bits::{
    bit_length_u32, bit_length_u64, bit_length_u128, magnitude_bits_i32, magnitude_bits_i64,
    magnitude_bits_i128, narrow_i64, narrow_i128, try_narrow_i64, try_narrow_i128,
};

/// Zero has no bits, which is the answer the shifts downstream depend on: a
/// magnitude of zero wants to be shifted all the way up, and `BITS - 0` would
/// say the opposite.
#[test]
fn zero_occupies_no_bits() {
    assert_eq!(bit_length_u32(0), 0);
    assert_eq!(bit_length_u64(0), 0);
    assert_eq!(bit_length_u128(0), 0);
    assert_eq!(magnitude_bits_i32(0), 0);
    assert_eq!(magnitude_bits_i64(0), 0);
    assert_eq!(magnitude_bits_i128(0), 0);
}

/// A power of two needs one more bit than the value below it, and that step is
/// exactly where an off-by-one in a normalizing shift shows up.
#[test]
fn each_power_of_two_takes_one_more_bit_than_its_predecessor() {
    for exponent in 0..u32::BITS {
        let value = 1u32 << exponent;
        assert_eq!(bit_length_u32(value), exponent + 1, "2^{exponent}");
        if exponent > 0 {
            assert_eq!(bit_length_u32(value - 1), exponent, "2^{exponent} - 1");
        }
    }
    for exponent in 0..u64::BITS {
        let value = 1u64 << exponent;
        assert_eq!(bit_length_u64(value), exponent + 1, "2^{exponent}");
        if exponent > 0 {
            assert_eq!(bit_length_u64(value - 1), exponent, "2^{exponent} - 1");
        }
    }
    for exponent in 0..u128::BITS {
        let value = 1u128 << exponent;
        assert_eq!(bit_length_u128(value), exponent + 1, "2^{exponent}");
        if exponent > 0 {
            assert_eq!(bit_length_u128(value - 1), exponent, "2^{exponent} - 1");
        }
    }
}

/// The whole width, for the value that fills it.
#[test]
fn all_ones_fills_the_word() {
    assert_eq!(bit_length_u32(u32::MAX), 32);
    assert_eq!(bit_length_u64(u64::MAX), 64);
    assert_eq!(bit_length_u128(u128::MAX), 128);
}

/// `MIN` is the value a negation would overflow on, and the one a magnitude
/// taken the obvious way gets wrong. It needs the full width.
#[test]
fn the_most_negative_value_needs_the_whole_word() {
    assert_eq!(magnitude_bits_i32(i32::MIN), 32);
    assert_eq!(magnitude_bits_i64(i64::MIN), 64);
    assert_eq!(magnitude_bits_i128(i128::MIN), 128);

    // `MIN + 1` is the neighbour that does have a positive twin, and it is one
    // bit narrower. Pinning the step is what makes the three answers above a
    // property of `MIN` alone rather than of every negative value: an
    // implementation that special-cases the sign instead of taking the
    // magnitude -- answering `BITS` whenever the value is below zero -- agrees
    // with this test at `MIN` and is wrong one step later.
    assert_eq!(magnitude_bits_i32(i32::MIN + 1), 31);
    assert_eq!(magnitude_bits_i64(i64::MIN + 1), 63);
    assert_eq!(magnitude_bits_i128(i128::MIN + 1), 127);
}

/// A magnitude is a magnitude: the sign does not change how wide it is, except
/// at `MIN`, which has no positive twin.
#[test]
fn sign_does_not_change_the_width() {
    for exponent in 0..31 {
        let value = 1i32 << exponent;
        assert_eq!(magnitude_bits_i32(value), magnitude_bits_i32(-value));
        assert_eq!(magnitude_bits_i32(value), exponent + 1, "2^{exponent}");
    }
    for exponent in 0..63 {
        let value = 1i64 << exponent;
        assert_eq!(magnitude_bits_i64(value), magnitude_bits_i64(-value));
        assert_eq!(magnitude_bits_i64(value), exponent + 1, "2^{exponent}");
    }
    for exponent in 0..127 {
        let value = 1i128 << exponent;
        assert_eq!(magnitude_bits_i128(value), magnitude_bits_i128(-value));
        assert_eq!(magnitude_bits_i128(value), exponent + 1, "2^{exponent}");
    }
}

/// The narrowing is the identity on everything that fits, and this is the range
/// where the callers spend all their time.
#[test]
fn narrowing_is_exact_inside_the_range() {
    for value in [
        0i64,
        1,
        -1,
        12345,
        -12345,
        i64::from(i32::MAX),
        i64::from(i32::MIN),
    ] {
        assert_eq!(i64::from(narrow_i64(value)), value);
        assert_eq!(try_narrow_i64(value).map(i64::from), Some(value));
        assert_eq!(
            i128::from(narrow_i128(i128::from(value))),
            i128::from(value)
        );
        assert_eq!(
            try_narrow_i128(i128::from(value)).map(i64::from),
            Some(value)
        );
    }
}

/// One step past each end, which is the boundary the saturating and the checked
/// answers disagree about -- and the only place they do.
#[test]
fn the_two_answers_differ_exactly_one_step_out() {
    let above = i64::from(i32::MAX) + 1;
    let below = i64::from(i32::MIN) - 1;

    assert_eq!(narrow_i64(above), i32::MAX);
    assert_eq!(narrow_i64(below), i32::MIN);
    assert_eq!(try_narrow_i64(above), None);
    assert_eq!(try_narrow_i64(below), None);

    assert_eq!(narrow_i128(i128::from(above)), i32::MAX);
    assert_eq!(narrow_i128(i128::from(below)), i32::MIN);
    assert_eq!(try_narrow_i128(i128::from(above)), None);
    assert_eq!(try_narrow_i128(i128::from(below)), None);
}

/// The extremes of the wide types, which is what a dot product of three
/// saturated components can actually reach.
#[test]
fn the_widest_intermediates_clamp_rather_than_wrap() {
    assert_eq!(narrow_i64(i64::MAX), i32::MAX);
    assert_eq!(narrow_i64(i64::MIN), i32::MIN);
    assert_eq!(narrow_i128(i128::MAX), i32::MAX);
    assert_eq!(narrow_i128(i128::MIN), i32::MIN);
    assert_eq!(try_narrow_i64(i64::MAX), None);
    assert_eq!(try_narrow_i128(i128::MIN), None);
}

/// Every function here is `const`, and this is what makes a regression to a
/// non-const implementation a compile error rather than a silent loss.
///
/// All ten are named, not a representative few: constness is a property of each
/// body separately, so pinning a subset pins only that subset. The values are
/// asserted rather than merely evaluated, which makes each line also a check
/// that const evaluation and the runtime path agree -- the two answers come from
/// the same body, but only the assertion says so.
#[test]
fn everything_is_usable_in_a_const() {
    const LENGTH_U32: u32 = bit_length_u32(0b1011);
    const LENGTH_U64: u32 = bit_length_u64(1 << 40);
    const LENGTH_U128: u32 = bit_length_u128(u128::MAX);
    const MAGNITUDE_I32: u32 = magnitude_bits_i32(i32::MIN);
    const MAGNITUDE_I64: u32 = magnitude_bits_i64(-1);
    const MAGNITUDE_I128: u32 = magnitude_bits_i128(i128::MIN);
    const CLAMPED_I64: i32 = narrow_i64(i64::MAX);
    const CLAMPED_I128: i32 = narrow_i128(i128::MIN);
    // `i64::from` is not const, so the widening here is a cast rather than the
    // `From` the rest of this file uses.
    const CHECKED_I64: Option<i32> = try_narrow_i64(i32::MAX as i64);
    const CHECKED_I128: Option<i32> = try_narrow_i128(1 << 100);

    assert_eq!(LENGTH_U32, 4);
    assert_eq!(LENGTH_U64, 41);
    assert_eq!(LENGTH_U128, 128);
    assert_eq!(MAGNITUDE_I32, 32);
    assert_eq!(MAGNITUDE_I64, 1);
    assert_eq!(MAGNITUDE_I128, 128);
    assert_eq!(CLAMPED_I64, i32::MAX);
    assert_eq!(CLAMPED_I128, i32::MIN);
    assert_eq!(CHECKED_I64, Some(i32::MAX));
    assert_eq!(CHECKED_I128, None);
}
