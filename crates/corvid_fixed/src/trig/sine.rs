//! The sine and its neighbours: the Taylor polynomial, the octant fold, and
//! the fast parabola.

use super::wide::{SIN_Q_ERROR, q_to_snorm_wide, sin_q_wide};
use super::{OCTANT, OCTANT_BITS, ONE, Q, TWO_PI, div_round, mulq, q_to_snorm};

/// Number of terms in the sine and cosine polynomials.
pub(super) const TERMS: usize = 7;

/// Builds the Taylor coefficients of a series in `x^2` with alternating signs.
///
/// `first` selects the series: 2 gives `1/(2k+1)!`, the coefficients of
/// `sin(x)/x`, and 1 gives `1/(2k)!`, the coefficients of `cos(x)`. They are
/// stored as reciprocals so evaluation needs no division, which is the entire
/// reason for precomputing them.
const fn taylor_coefficients(first: u64) -> [i64; TERMS] {
    let mut coefficients = [0; TERMS];
    coefficients[0] = ONE;
    let mut factorial: u64 = 1;
    let mut n = first;
    let mut i = 1;
    while i < TERMS {
        factorial *= n * (n + 1);
        let magnitude = ONE / (factorial as i64);
        coefficients[i] = if i % 2 == 1 { -magnitude } else { magnitude };
        n += 2;
        i += 1;
    }
    coefficients
}

/// Coefficients of `sin(x)/x` as a polynomial in `x^2`.
pub(super) const SIN_COEFFICIENTS: [i64; TERMS] = taylor_coefficients(2);

/// Coefficients of `cos(x)` as a polynomial in `x^2`.
pub(super) const COS_COEFFICIENTS: [i64; TERMS] = taylor_coefficients(1);

/// Evaluates a polynomial in `x2` by Horner's method, from the top down.
const fn horner(x2: i64, coefficients: &[i64; TERMS]) -> i64 {
    let mut i = TERMS - 1;
    let mut acc = coefficients[i];
    while i > 0 {
        i -= 1;
        acc = coefficients[i] + mulq(x2, acc);
    }
    acc
}

/// Evaluates `sin(x)` in Q60 for `0 <= x <= pi/4`.
///
/// Seven terms. The first omitted term is `x^15 / 15!`, which peaks at 2.0e-14
/// over this interval -- four orders of magnitude below the last bit of the widest
/// output type.
const fn sin_poly(x: i64) -> i64 {
    mulq(x, horner(mulq(x, x), &SIN_COEFFICIENTS))
}

/// Evaluates `cos(x)` in Q60 for `0 <= x <= pi/4`.
///
/// Seven terms. The first omitted term is `x^14 / 14!`, which peaks at 3.9e-13.
/// Exactly `1` at zero, since Horner bottoms out on the leading coefficient.
const fn cos_poly(x: i64) -> i64 {
    horner(mulq(x, x), &COS_COEFFICIENTS)
}

/// Returns `sin(2*pi * phase / 2^32)` in Q60.
///
/// The phase is folded into the first octant using the eightfold symmetry of the
/// sine, which bounds the polynomial argument by `pi/4` where the Taylor series
/// is at its most accurate. The results at multiples of a quarter turn are
/// exactly `-1`, `0`, and `1`.
pub(crate) const fn sin_q(phase: u32) -> i64 {
    let octant = phase >> OCTANT_BITS;
    let offset = phase & (OCTANT - 1);

    // Odd octants run backwards: measure from the far end instead.
    let folded = if octant % 2 == 1 {
        OCTANT - offset
    } else {
        offset
    };
    let x = (((folded as i128) * (TWO_PI as i128)) >> 32) as i64;

    // Octants 1, 2, 5 and 6 sit either side of a peak, where sine mirrors into
    // cosine; the rest sit either side of a zero crossing.
    let magnitude = if matches!(octant, 1 | 2 | 5 | 6) {
        cos_poly(x)
    } else {
        sin_poly(x)
    };

    if octant >= 4 { -magnitude } else { magnitude }
}

/// Returns `cos(2*pi * phase / 2^32)` in Q60.
pub(crate) const fn cos_q(phase: u32) -> i64 {
    sin_q(phase.wrapping_add(1 << 30))
}

/// Returns `sin(2*pi * phase / 2^32)` in Q30 using a cheap approximation.
///
/// A parabola through the sine's zeros and peak, corrected by a second parabola
/// in its own output -- the classic `0.775 * y + 0.225 * y * |y|` refinement. The
/// values at multiples of a quarter turn are exact.
///
/// # Thirty-two bits
///
/// Unlike the rest of this module, every intermediate here fits an `i32` and
/// every operation has a `WGSL` equivalent, so the function transcribes directly
/// into a shader. Three consequences shaped the code:
///
/// - There is no widening multiply. `WGSL` has no 64-bit integer and no
///   `mulExtended`, so a 32x32-to-64 product is simply unavailable and every
///   product below has to be bounded to 31 bits by construction.
/// - Signed overflow is implementation-defined in `WGSL`, so anything that could
///   reach `2^31` stays in `u32`, where wrapping is specified.
/// - Q16 is the largest scale the parabola can use. `z * (2^k - z)` peaks at
///   `2^(2k-2)`, and `k = 16` is the last value keeping that under `2^31`. It
///   lands the result in Q30, so a 31-bit multiply yields 31 bits of answer with
///   nothing wasted.
pub(crate) const fn sin_fast_q30(phase: u32) -> i32 {
    /// Half of the phase space.
    const HALF_TURN: u32 = 1 << 31;
    /// A quarter of the phase space, where the sine peaks.
    const QUARTER_TURN: i32 = 1 << 30;
    /// Fractional bits of the folded phase, which is measured in half-turns.
    const Z_BITS: u32 = 16;
    /// One half-turn in the parabola's units.
    const Z_ONE: i32 = 1 << Z_BITS;
    /// Bits of phase discarded on the way down to `Z_BITS`.
    const Z_SHIFT: u32 = 31 - Z_BITS;
    /// Weight of the correction term, `0.225` as `9/40`, in Q15.
    ///
    /// The linear weight is implicit. Writing the refinement as `y` plus a
    /// correction rather than as a weighted sum means `0.775` never has to be
    /// represented, and the two weights never have to be made to sum to one.
    const W_SQUARE: i32 = 9 * (1 << 15) / 40;

    // Fold about the peak, as a distance from it rather than a comparison
    // against it. The parabola is symmetric there, so this costs no accuracy and
    // buys exactness: the result comes out an exactly odd function of the phase,
    // and the cosine an exactly even one, which a one-sided shift of the phase
    // cannot manage. Written with `abs` rather than the equivalent comparison
    // because the phase is unpredictable, so the branch it compiles to is one
    // the processor cannot call -- measured 20% off the whole function -- and a
    // shader would pay more again for the divergence.
    let half = (phase & (HALF_TURN - 1)) as i32;
    let folded = QUARTER_TURN - (half - QUARTER_TURN).abs();

    // Half-turns in Q16, rounded. `folded` never exceeds a quarter turn, so `z`
    // never exceeds `2^15` and the product never exceeds `2^30`.
    let z = (folded + (1 << (Z_SHIFT - 1))) >> Z_SHIFT;
    let parabola = z * (Z_ONE - z);

    // `y^2` in Q30. Halving the scale first is what keeps the square inside 31
    // bits: two Q15 values multiply to a Q30 one exactly.
    let root = (parabola + (1 << 14)) >> 15;
    let delta = root * root - parabola;

    // `0.225 * (y^2 - y)`. The delta is bounded by a quarter, so dropping ten of
    // its bits leaves the correction good to 1e-7 while making room for the
    // weight. At the peak the delta is exactly zero, which is what keeps the
    // sine of a quarter turn exactly one.
    let magnitude = parabola + ((W_SQUARE * (delta >> 10)) >> 5);

    // Negate over the second half turn, again without branching: `sign` is all
    // ones there and zero elsewhere, and `(m ^ sign) - sign` is `-m` exactly
    // when it is.
    let sign = (phase as i32) >> 31;
    (magnitude ^ sign) - sign
}

/// Returns `sin(2*pi * phase / 2^32)` as a signed-normalized bit pattern,
/// correctly rounded.
///
/// Only the 32-bit output does any work here. Its Q60 answer stands unless it
/// falls within [`SIN_Q_ERROR`] of a rounding boundary, where it cannot be
/// trusted to land on the right side; those recompute in Q100. The window covers
/// `2^-8` of the phase space, so about one call in 256 takes the slow path, which
/// costs the 32-bit sine roughly a tenth of its time overall. The narrower
/// outputs skip the test entirely -- see the comment below.
///
/// Inlining is not optional here. `max` is a constant at every call site, and the
/// width test turns into nothing at all once it is one; left out of line, the
/// narrow types pay for a decision that was made when their type was chosen.
#[inline]
pub(crate) const fn sin_snorm(phase: u32, max: i64) -> i64 {
    let v = sin_q(phase);

    // Outputs of sixteen bits or fewer never need the exact tier. The Q60 error
    // is 2^-44, nine orders of magnitude below their last bit, and their whole
    // domain is small enough to prove it rather than argue it -- which
    // `the_narrow_widths_never_need_the_exact_tier` does, by walking every phase
    // either type can hold and finding Q60 and Q100 already in agreement. `max`
    // is a constant at every call site, so this test costs them nothing at all.
    if max <= u16::MAX as i64 {
        return q_to_snorm(v, max);
    }

    // Whether the scaled value sits near a rounding boundary is decided entirely
    // by the low 60 bits of `|v| * max`, and 64-bit arithmetic carries those
    // exactly. So the test runs beside the 128-bit product rather than after it,
    // and everything that wraps above bit 60 is discarded by the mask anyway.
    //
    // A boundary is dangerous from either side. Shifting up by the bound before
    // masking folds both sides into one unsigned comparison: the window that
    // straddled zero becomes `0 .. 2 * bound`.
    let bound = (SIN_Q_ERROR as u64) * (max as u64);
    let residue = v
        .unsigned_abs()
        .wrapping_mul(max as u64)
        .wrapping_add(1 << (Q - 1))
        .wrapping_add(bound)
        & ((1_u64 << Q) - 1);
    if residue < 2 * bound {
        return sin_snorm_wide(phase, max);
    }

    q_to_snorm(v, max)
}

/// The exact tier, as a separate function so it stays out of the hot path.
///
/// Marked cold and never-inlined deliberately: twelve `i128` Horner steps inlined
/// into [`sin_snorm`] cost more in register pressure at every call site than the
/// branch saves on the one call in 256 that gets here.
#[cold]
#[inline(never)]
const fn sin_snorm_wide(phase: u32, max: i64) -> i64 {
    q_to_snorm_wide(sin_q_wide(phase), max)
}

/// Returns `cos(2*pi * phase / 2^32)` as a signed-normalized bit pattern,
/// correctly rounded.
#[inline]
pub(crate) const fn cos_snorm(phase: u32, max: i64) -> i64 {
    sin_snorm(phase.wrapping_add(1 << 30), max)
}

/// Returns `tan(2*pi * phase / 2^32)` as an [`I24F8`](crate::I24F8) bit pattern.
///
/// Saturates at the poles, where the cosine is exactly zero.
pub(crate) const fn tan_i24f8(phase: u32) -> i32 {
    let sin = sin_q(phase);
    let cos = cos_q(phase);

    // Dividing by `cos >> 8` yields `sin * 256 / cos` while keeping both operands
    // inside i64 -- `sin << 8` would not fit. The shift leaves the divisor 52
    // significant bits, so the quotient stays accurate well past I24F8's
    // resolution everywhere the result is not already saturating.
    let divisor = cos >> 8;
    if divisor == 0 {
        return if sin >= 0 { i32::MAX } else { i32::MIN };
    }
    let scaled = div_round(sin, divisor);
    if scaled > i32::MAX as i64 {
        i32::MAX
    } else if scaled < i32::MIN as i64 {
        i32::MIN
    } else {
        scaled as i32
    }
}
