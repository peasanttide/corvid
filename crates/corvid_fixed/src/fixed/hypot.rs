//! The integer hypotenuse shared by the fixed-point family.
//!
//! `sqrt(x^2 + y^2)` is the one composition fixed point gets for free. A value
//! is its bit pattern over a fixed scale, so with `a` and `b` the bit patterns
//! of `x` and `y` the answer is `sqrt(a^2 + b^2) / 2^F`: the integer square
//! root of the summed squares **is** the result's bit pattern, and there is no
//! rescaling step for a rounding to hide in.
//! [`sqrt`](crate::I16F16::sqrt) has to shift its input up by `2^F` before
//! rooting it and this does not, which is why an exact hypotenuse costs less
//! than the square root of a sum that a caller would otherwise write.
//!
//! The sum itself never overflows the widened type each fixed-point type
//! already carries for its multiply. Two squares of an `N`-bit signed integer
//! come to at most `2 * (2^(N-1))^2`, which is `2^(2N-1)`, and the widened type
//! has `2N` bits. So [`I16F16`](crate::I16F16) reaches `u64` and stops there:
//! the 32-bit types never touch 128-bit arithmetic, and only
//! [`I48F16`](crate::I48F16), whose bit pattern is already an `i64`, does.
//!
//! # Why not `isqrt`
//!
//! [`u64::isqrt`] costs what its input's magnitude costs, and a sum of squares
//! is the widest thing this crate asks a `u64` root for: two `i32` bit patterns
//! reach `2^63`, where the library routine is at its slowest and this crate's
//! operands are at their most ordinary. [`root_u64`] is a reciprocal-root Newton estimate
//! instead -- sharing its narrow phase with
//! [`rsqrt`](super::rsqrt::reciprocal_root_q30) -- and what it costs does not
//! depend on its input at all. That is worth about 2x on both
//! [`I16F16`](crate::I16F16) and [`I48F16`](crate::I48F16) over full-range
//! legs, widening as the operands do and shrinking to nothing for legs of a
//! byte or two; `cargo bench -p corvid_fixed --bench scalar` measures both
//! against the `isqrt` they replace.
//!
//! Newton lands the estimate near the answer rather than on it, so both kernels
//! here end the same way [`rsqrt_bits`](super::rsqrt::rsqrt_bits) does: an
//! exact integer comparison that costs one multiply and leaves the answer
//! **correctly rounded** rather than merely close.
//!
//! # Why there is no approximate tier
//!
//! [`rsqrt_fast`](crate::I16F16::rsqrt_fast) is faster than
//! [`rsqrt`](crate::I16F16::rsqrt) because it skips 128-bit multiplies that the
//! exact form genuinely needs. A hypotenuse has none to skip. The 32-bit-clean
//! shape -- shift both legs down until the sum of squares fits `u32`, root it
//! narrow, shift back -- was written and measured, and it comes out *slower*
//! than the exact kernel at every operand width while being `9.8e-5` out,
//! because what it buys with the accuracy it gives up is a `u32::isqrt` on a
//! normalized operand, which is that routine's worst case. So there is no
//! `hypot_fast`, and this is the note that says the option was measured rather
//! than missed.

use super::rsqrt::reciprocal_root_q30;

/// How far the residual is shifted down before it meets the reciprocal root.
///
/// The refinement multiplies a residual by `q`, and the two of them have to fit
/// one operand between them. The residual is `n - r^2`, which the estimate's
/// own accuracy bounds: under `2^37` at the 64-bit width, and under `2^99` at
/// the 128-bit one, where the first pass starts from a root whose low half is
/// still an estimate. Shifting down by this much leaves room for `q`'s 31 bits
/// in either.
///
/// What the shift costs is `2^18` of a numerator whose scale is `2 * sqrt(n)`,
/// so under `2^-13` of a last bit at the narrow width and far less at the wide
/// one -- well inside the one last bit the exact correction after it absorbs.
const RESIDUAL_SHIFT: u32 = 18;

/// `round(sqrt(sum))`, the hypotenuse's bit pattern, for a sum of two squares
/// that fits `u64`.
///
/// Every type whose bit pattern is 32 bits or narrower comes here, because
/// `2^63` is where two squared `i32` bit patterns stop. `sum` may be `2^63`
/// exactly, which is what `i32::MIN` in both legs comes to.
#[inline]
pub(super) const fn root_u64(sum: u64) -> u64 {
    if sum == 0 {
        return 0;
    }

    // 1. Normalize into `[2^61, 2^63]`, the binade pair the shared seed is
    //    fitted to, by an even shift so that halving it below is exact. The
    //    saturation is for the one input that has no leading zero at all.
    let shift = sum.leading_zeros().saturating_sub(1) & !1;
    let n = sum << shift;

    // 2. `q` is `2^30 / sqrt(n)` at Q30, so `n / sqrt(n)` -- the root itself --
    //    is `(n >> 32) * q` brought back down. Taking `n` down to Q30 for the
    //    multiply is what keeps the product inside `u64`, and it is also what
    //    makes this an estimate rather than an answer: the 32 bits it drops are
    //    worth about three last bits of the root.
    let narrow = n >> 32;
    let q = reciprocal_root_q30(narrow);
    let mut root = ((narrow * q) >> 30) << 1;

    // 3. One residual step, against the whole of `n` rather than the top half
    //    of it, which is what pays those three bits back. `dr` is
    //    `(n - r^2) / (2 sqrt(n))` and `1 / (2 sqrt(n))` is `q >> 62`, so the
    //    step is a subtraction, a multiply and a shift. `r^2` reaches `2^63`
    //    and `n` reaches `2^63`, and neither fits `i64` -- hence the sign
    //    carried alongside an unsigned magnitude.
    let square = root * root;
    let high = square > n;
    let residual = (if high { square - n } else { n - square }) >> RESIDUAL_SHIFT;
    let delta = (residual * q) >> (62 - RESIDUAL_SHIFT);
    // `delta` is the estimate's own error, a few tens against a root of at
    // least `2^30`, so the subtraction stays well clear of zero.
    root = if high { root - delta } else { root + delta };

    // 4. Undo the normalization. `n` is `sum << shift` for an even `shift`, so
    //    `sqrt(sum)` is `sqrt(n) >> (shift / 2)` exactly, and flooring the
    //    shifted value is flooring the quotient.
    root >>= shift / 2;

    // 5. Correct exactly. The estimate is within one either way, so one
    //    comparison in each direction settles it; at most one of them fires,
    //    and on most inputs neither does.
    if root * root > sum {
        root -= 1;
    }
    if (root + 1) * (root + 1) <= sum {
        root += 1;
    }

    // 6. Round to nearest. The true root is past the halfway point exactly when
    //    the remainder exceeds the floor, and it is never *on* the halfway
    //    point: the square root of a non-square integer is irrational.
    if sum - root * root > root {
        root + 1
    } else {
        root
    }
}

/// `round(sqrt(sum))` for a sum of two squares that needs `u128`.
///
/// Only [`I48F16`](crate::I48F16) reaches this, and only for legs too far apart
/// for a `u64` to hold their squares. Anything nearer goes to [`root_u64`],
/// which is exact over that range already and does not pay for the width.
#[inline]
pub(super) const fn root_u128(sum: u128) -> u128 {
    if sum <= 1 << 63 {
        return root_u64(sum as u64) as u128;
    }

    // The same six steps as [`root_u64`], two binade pairs further up. `n`
    // lands in `[2^125, 2^127]`, its Q30 window sits 96 bits along rather than
    // 32, and the estimate is placed 33 bits up rather than one.
    let shift = sum.leading_zeros().saturating_sub(1) & !1;
    let n = sum << shift;
    let narrow = (n >> 96) as u64;
    let q = reciprocal_root_q30(narrow);
    let mut root = (((narrow * q) >> 30) as u128) << 33;

    // Twice, because a residual step only doubles the estimate's significant
    // bits: the narrow phase carries about 30 of them and this answer wants 63.
    let mut pass = 0;
    while pass < 2 {
        let square = root * root;
        let high = square > n;
        let residual = (if high { square - n } else { n - square }) >> RESIDUAL_SHIFT;
        let delta = (residual * q as u128) >> (94 - RESIDUAL_SHIFT);
        root = if high { root - delta } else { root + delta };
        pass += 1;
    }

    root >>= shift / 2;
    if root * root > sum {
        root -= 1;
    }
    if (root + 1) * (root + 1) <= sum {
        root += 1;
    }
    if sum - root * root > root {
        root + 1
    } else {
        root
    }
}
