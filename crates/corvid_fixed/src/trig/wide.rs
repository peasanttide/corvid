//! The Q100 refinement, for the phases where Q60 cannot say which way the last
//! bit of the widest output rounds.

use super::{OCTANT, OCTANT_BITS};

/// Fractional bits in the extended representation used by the exact tier.
pub(super) const Q_WIDE: u32 = 100;

/// `1.0` in the extended representation.
pub(super) const ONE_WIDE: i128 = 1 << Q_WIDE;

/// Low 64 bits of a `u128`.
const LIMB: u128 = u64::MAX as u128;

/// Multiplies two `i128` values and shifts the full 256-bit product right by
/// `shift`, truncating toward zero.
///
/// `shift` must lie in `1 ..= 127`, and the shifted result must fit an `i128`.
///
/// Rust has no 256-bit integer, so the product is assembled from four 64-bit
/// limbs by hand. Signs are stripped first and reapplied at the end, which keeps
/// the limb arithmetic unsigned and makes the truncation symmetric about zero --
/// the same convention [`q_to_snorm`] already rounds under.
pub(super) const fn mul_shift(a: i128, b: i128, shift: u32) -> i128 {
    let negative = (a < 0) != (b < 0);
    let (a, b) = (a.unsigned_abs(), b.unsigned_abs());

    let (a_high, a_low) = ((a >> 64) as u64, a as u64);
    let (b_high, b_low) = ((b >> 64) as u64, b as u64);
    let low_low = (a_low as u128) * (b_low as u128);
    let low_high = (a_low as u128) * (b_high as u128);
    let high_low = (a_high as u128) * (b_low as u128);
    let high_high = (a_high as u128) * (b_high as u128);

    // The two cross terms straddle the 64-bit boundary, so each is split: the low
    // half joins the middle column, the high half carries into the top word.
    let middle = (low_low >> 64) + (low_high & LIMB) + (high_low & LIMB);
    let low = (low_low & LIMB) | ((middle & LIMB) << 64);
    let high = high_high + (low_high >> 64) + (high_low >> 64) + (middle >> 64);

    let magnitude = if shift >= 128 {
        high >> (shift - 128)
    } else {
        (low >> shift) | (high << (128 - shift))
    };

    if negative {
        -(magnitude as i128)
    } else {
        magnitude as i128
    }
}

/// Evaluates `atan(z)` in Q100 by the Gregory series. See [`atan_series`].
const fn atan_series_wide(z: i128) -> i128 {
    let z2 = mul_shift(z, z, Q_WIDE);
    let mut power = z;
    let mut k: i128 = 0;
    let mut acc: i128 = 0;
    while power != 0 {
        let term = power / (2 * k + 1);
        if k % 2 == 0 {
            acc += term;
        } else {
            acc -= term;
        }
        power = mul_shift(power, z2, Q_WIDE);
        k += 1;
    }
    acc
}

/// Pi in Q100, from Machin's formula. See [`PI`].
pub(super) const PI_WIDE: i128 =
    16 * atan_series_wide(ONE_WIDE / 5) - 4 * atan_series_wide(ONE_WIDE / 239);

/// A full turn in Q100 radians.
const TWO_PI_WIDE: i128 = 2 * PI_WIDE;

/// Number of terms in the extended-precision sine and cosine polynomials.
///
/// Twelve leaves the first omitted term at `x^25 / 25!` for the sine and
/// `x^24 / 24!` for the cosine, which peak at 1.5e-28 and 4.9e-27 over the first
/// octant. The cosine is the weaker of the two and sets the tier's accuracy.
pub(super) const TERMS_WIDE: usize = 12;

/// Builds Q100 Taylor coefficients. See [`taylor_coefficients`].
const fn taylor_coefficients_wide(first: u128) -> [i128; TERMS_WIDE] {
    let mut coefficients = [0; TERMS_WIDE];
    coefficients[0] = ONE_WIDE;
    let mut factorial: u128 = 1;
    let mut n = first;
    let mut i = 1;
    while i < TERMS_WIDE {
        factorial *= n * (n + 1);
        let magnitude = ONE_WIDE / (factorial as i128);
        coefficients[i] = if i % 2 == 1 { -magnitude } else { magnitude };
        n += 2;
        i += 1;
    }
    coefficients
}

/// Q100 coefficients of `sin(x)/x` as a polynomial in `x^2`.
pub(super) const SIN_COEFFICIENTS_WIDE: [i128; TERMS_WIDE] = taylor_coefficients_wide(2);

/// Q100 coefficients of `cos(x)` as a polynomial in `x^2`.
pub(super) const COS_COEFFICIENTS_WIDE: [i128; TERMS_WIDE] = taylor_coefficients_wide(1);

/// Evaluates a Q100 polynomial in `x2` by Horner's method, from the top down.
const fn horner_wide(x2: i128, coefficients: &[i128; TERMS_WIDE]) -> i128 {
    let mut i = TERMS_WIDE - 1;
    let mut acc = coefficients[i];
    while i > 0 {
        i -= 1;
        acc = coefficients[i] + mul_shift(x2, acc, Q_WIDE);
    }
    acc
}

/// Returns `sin(2*pi * phase / 2^32)` in Q100.
///
/// The same octant folding and the same two polynomials as [`sin_q`], carried out
/// at the wider scale. The argument reduction goes through [`mul_shift`] rather
/// than a plain product because a 29-bit offset against a Q100 turn overshoots
/// `i128` by five bits before the shift brings it back.
pub(super) const fn sin_q_wide(phase: u32) -> i128 {
    let octant = phase >> OCTANT_BITS;
    let offset = phase & (OCTANT - 1);

    let folded = if octant % 2 == 1 {
        OCTANT - offset
    } else {
        offset
    };
    let x = mul_shift(folded as i128, TWO_PI_WIDE, 32);
    let x2 = mul_shift(x, x, Q_WIDE);

    let magnitude = if matches!(octant, 1 | 2 | 5 | 6) {
        horner_wide(x2, &COS_COEFFICIENTS_WIDE)
    } else {
        mul_shift(x, horner_wide(x2, &SIN_COEFFICIENTS_WIDE), Q_WIDE)
    };

    if octant >= 4 { -magnitude } else { magnitude }
}

/// Guard bits kept below the output's last bit while rounding a Q100 value.
///
/// [`mul_shift`] truncates, so the product is taken this many bits finer than the
/// answer needs and the discarded remainder is worth at most `2^-60` of a last
/// bit -- far below the polynomial's own error, and far below the closest a true
/// sine comes to a rounding boundary.
const WIDE_GUARD: u32 = 60;

/// Scales a Q100 value in `[-1, 1]` to a signed-normalized bit pattern.
///
/// `max` is the bit pattern denoting `1.0`. Halfway cases round away from zero,
/// matching [`q_to_snorm`].
pub(super) const fn q_to_snorm_wide(v: i128, max: i64) -> i64 {
    let scaled = mul_shift(v, max as i128, Q_WIDE - WIDE_GUARD);
    let half = 1_i128 << (WIDE_GUARD - 1);
    if v >= 0 {
        ((scaled + half) >> WIDE_GUARD) as i64
    } else {
        -(((-scaled + half) >> WIDE_GUARD) as i64)
    }
}

/// Worst-case error of [`sin_q`], in Q60 units.
///
/// Dominated by the cosine polynomial's first omitted term, `x^14 / 14!`, which
/// peaks at 3.9e-13 over the first octant -- about `2^18.8` here. The rest is
/// small change: seven truncations inside [`horner`], one in the argument
/// reduction, and the error carried by [`TWO_PI`] itself, together under thirty
/// units. Rounding up to `2^20` leaves better than a factor of two in hand.
pub(super) const SIN_Q_ERROR: i64 = 1 << 20;
