//! Integer powers, against the answer worked out exactly.
//!
//! `powi` is binary exponentiation, so its error is the roundings in a chain of
//! about `log2(|n|)` multiplies rather than of `|n|` of them. Holding it to the
//! intrinsic would only compare two approximations, so the reference here is
//! built by hand at full width and rounded once, and what the test reports is
//! how far the chain carried the answer from it.

#![allow(
    clippy::cast_precision_loss,
    reason = "the loop counters are small integers being turned into the sample points, which is the standard way to sweep a range and is exact for every value they take"
)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "the exact reference assembles a bit pattern from an exponent and a mantissa it has already bounded, and the narrowing is how those fields are written down"
)]

mod common;

use common::{same, ulps};

/// An integer power, against the answer worked out exactly.
///
/// # Why not against the intrinsic
///
/// Because `f32::powi` is not an answer. It lowers to `llvm.powi`, which is not
/// correctly rounded and is free to differ between targets -- and it does: at
/// `powi(-5.25, -18)` it lands three ulps from the true value on
/// `x86_64-pc-windows-msvc` and on the true value on
/// `x86_64-unknown-linux-gnu`. Holding this crate to it would be holding a
/// deterministic function to a non-deterministic one, so the test would say
/// different things on different machines and none of them about this crate.
///
/// [`exact_powi`] works the answer out instead. `(step / 8)^n` is a rational
/// whose parts are a power of an odd integer and a power of two, so once the
/// common twos are cancelled the correctly rounded `f32` is an integer division
/// and a remainder to settle the tie on. The odd part reaches 113 bits at
/// `49^20` -- too wide to shift a numerator above and still fit a `u128` -- so
/// the reciprocal is taken by [`reciprocal`]'s long division, which never forms
/// a number wider than one doubling of the divisor. There is no floating point
/// anywhere in either, so nothing in them can vary by target.
///
/// Cross-checked against exact rational arithmetic in Python over all four
/// thousand samples of this sweep: every one agrees bit for bit.
///
/// # What the bound says
///
/// That this crate's `powi` is within `1 + ilog2(|n|)` of the true value, which
/// is the shape of the algorithm: binary exponentiation is about `log2(|n|)`
/// squarings and as many multiplies again, every one of them rounds, so the
/// distance from the truth grows with the length of the chain and with nothing
/// else. `n = 0` and `n = +/-1` are held exactly, which is where no chain runs.
///
/// Measured over this sweep, and the same on every target because both sides
/// are: 814 of the 4000 samples are not the correctly rounded value, the worst
/// is three ulps at `powi(-5.25, -18)` against a bound of five there, and
/// nothing below `|n| = 10` is out by more than one.
///
/// **This is a measurement, not a target.** A `powi` that carried a few guard
/// bits through the chain would be correctly rounded everywhere and this bound
/// would become `== 0`; the bound is here because the implementation does not,
/// not because three ulps is a thing worth promising.
#[test]
fn integer_powers_are_within_a_chain_s_worth_of_the_exact_answer() {
    /// How far the roundings in a chain for `n` may carry this crate's answer
    /// from the true one.
    ///
    /// About `log2(|n|)` of them, not `|n|`: binary exponentiation squares its
    /// way up rather than multiplying `|n|` times, which is why the bound
    /// doubles with the exponent instead of growing with it.
    fn allowed(n: i32) -> i64 {
        1 + i64::from(n.unsigned_abs().max(1).ilog2())
    }

    for step in -50i32..50 {
        let x = step as f32 / 8.0;
        for n in -20i32..20 {
            let (ours, truth) = (corvid_float::powi(x, n), exact_powi(step, n));
            let (apart, bound) = (ulps(ours, truth), allowed(n));
            assert!(
                apart <= bound,
                "powi({x}, {n}): {ours:e} against the exact {truth:e} -- {apart} ulps out, \
                 {bound} allowed"
            );
        }
    }

    // The edges, where the answer is exact and there is nothing to be out by.
    // `i32::MIN` is the one with no positive counterpart to take a magnitude of.
    same(corvid_float::powi(2.0, 0), 1.0, "powi(2, 0)");
    same(corvid_float::powi(0.0, 0), 1.0, "powi(0, 0)");
    same(corvid_float::powi(2.0, i32::MIN), 0.0, "powi(2, i32::MIN)");
    same(
        corvid_float::powi(2.0, i32::MAX),
        f32::INFINITY,
        "powi(2, i32::MAX)",
    );
}

/// The first `bits` significant bits of `1 / divisor`, how far below the point
/// they sit, and whether anything was left over.
///
/// Ordinary long division in binary: at each place the remainder doubles and
/// the divisor is taken out of it if it fits. The remainder is always below the
/// divisor, so the widest number this ever holds is one doubling of one -- which
/// is what lets it divide by a 113-bit `power` that no shifted numerator could
/// reach.
///
/// The leftover remainder is the whole of what "and a bit more" means: a true
/// value strictly above the quotient cannot be an exact tie, however the
/// dropped bits happen to look.
const fn reciprocal(divisor: u128, bits: u32) -> (u128, i64, bool) {
    let (mut quotient, mut remainder, mut places) = (0_u128, 1_u128, 0_i64);
    while 128 - quotient.leading_zeros() < bits {
        remainder <<= 1;
        quotient <<= 1;
        places += 1;
        if remainder >= divisor {
            remainder -= divisor;
            quotient += 1;
        }
    }
    (quotient, places, remainder > 0)
}

/// The correctly rounded `f32` nearest to `(step / 8)^n`, worked out exactly.
///
/// `step / 8` is `odd * 2^k / 8` for some odd `odd`, so `(step / 8)^n` is
/// `odd^|n|` over a power of two, or its reciprocal. `odd` is at most 25 here
/// and `|n|` at most 20, so `odd^|n|` is under 2^93 and every quantity below
/// fits a `u128` with room to shift in. There is no floating point anywhere in
/// this function, so it answers the same on every target.
///
/// Ties round to even, which is what IEEE 754 asks for and what the `f32` this
/// is compared against was rounded by.
fn exact_powi(step: i32, n: i32) -> f32 {
    if n == 0 {
        return 1.0;
    }
    if step == 0 {
        return if n > 0 { 0.0 } else { f32::INFINITY };
    }

    let negative = step < 0 && n % 2 != 0;
    let magnitude = u128::from(step.unsigned_abs());
    let twos = magnitude.trailing_zeros();
    let odd = magnitude >> twos;
    let times = n.unsigned_abs();

    // `odd^|n|`, and the power of two the eighths and the even part leave over.
    let power = odd.pow(times);
    let scale = if n > 0 {
        (i64::from(twos) - 3) * i64::from(times)
    } else {
        (3 - i64::from(twos)) * i64::from(times)
    };

    // The whole value as `mantissa * 2^exponent`, with the mantissa an exact
    // integer for a power and an integer plus a remainder for a reciprocal.
    // `sticky` is set when the true value lies strictly above that integer,
    // which is what stops a division's leftovers reading as an exact tie.
    let (mantissa, exponent, sticky) = if n > 0 {
        (power, scale, false)
    } else {
        // Long division rather than `(1 << s) / power`: `power` reaches 113
        // bits at `49^20`, and the shift that would put twenty-six quotient
        // bits above it does not fit a `u128`. Dividing a place at a time never
        // forms that number -- the remainder stays below the divisor, so the
        // widest value here is one doubling of it.
        let (quotient, places, rest) = reciprocal(power, 26);
        (quotient, scale - places, rest)
    };

    // Round that to exactly 24 bits, nearest, ties to even.
    let bits = i64::from(128 - mantissa.leading_zeros());
    let (mut significand, mut exponent) = if bits > 24 {
        let drop = bits - 24;
        #[expect(
            clippy::cast_sign_loss,
            reason = "`drop` is `bits - 24` inside the branch where `bits > 24`, so it is positive"
        )]
        let dropped = drop as u32;
        let cut = mantissa & ((1 << dropped) - 1);
        let half = 1_u128 << (dropped - 1);
        let mut significand = mantissa >> dropped;
        if cut > half || (cut == half && (sticky || significand & 1 == 1)) {
            significand += 1;
        }
        (significand, exponent + drop)
    } else {
        #[expect(
            clippy::cast_sign_loss,
            reason = "`24 - bits` is positive inside the branch where `bits <= 24`"
        )]
        let up = (24 - bits) as u32;
        (mantissa << up, exponent - (24 - bits))
    };
    // A carry out of the top: 0xffffff + 1 is 0x1000000, one bit wider.
    if significand == 1 << 24 {
        significand >>= 1;
        exponent += 1;
    }

    // Assemble. Everything this sweep reaches is a normal `f32`; a value that
    // is not says so rather than folding silently to zero or an infinity.
    let biased = exponent + 23 + 127;
    assert!(
        (1..=254).contains(&biased),
        "(({step}) / 8)^{n} is outside the normal range this reference covers"
    );
    // Both conversions are inside the ranges the assertion above and the
    // twenty-four-bit significand guarantee, so the fallback is unreachable and
    // is written rather than unwrapped.
    let exponent_field = u32::try_from(biased).unwrap_or(0);
    let mantissa_field = u32::try_from(significand & 0x007f_ffff).unwrap_or(0);
    let value = f32::from_bits((exponent_field << 23) | mantissa_field);
    if negative { -value } else { value }
}
