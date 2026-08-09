//! Integer-only trigonometry backing the [angle](crate::angle) and
//! [pitch](crate::pitch) types.
//!
//! Everything here is `const` and uses only integer arithmetic, so results are
//! bit-identical on every target. No floating-point value is produced, consumed,
//! or even written down: every constant in this file, pi included, is derived by
//! integer arithmetic at compile time, and no lookup table is baked into the
//! binary as literal data.
//!
//! # Internal representation
//!
//! Values are `i64` in Q60 -- an integer `v` denotes `v / 2^60`. That leaves 60
//! fractional bits against the 4.7e-10 resolution of
//! [`Signed32`](crate::Signed32), so rounding noise in the pipeline lands about
//! nine orders of magnitude below the last bit of the widest output.
//!
//! `i64` rather than `i128` is a deliberate performance choice, and the reason
//! [`mulq`] looks the way it does. Widening two `i64`s to `i128` for a single
//! product compiles to one multiply-high instruction pair; doing the *whole*
//! computation in `i128` costs three multiplies per product, and an `i128`
//! division by a constant becomes a call into `__divti3`. Measured on aarch64,
//! moving the polynomial from `i128` arithmetic with divisions to `i64`
//! arithmetic with reciprocal coefficients took a sine from 85 ns to under 8 ns.
//! `cargo bench -p corvid_fixed --bench trig` reproduces the comparison.
//!
//! `i128` earns its keep in two places: [`asin_bits`] at 32-bit output, where the
//! square root needs a 120-bit radicand, and the exact tier below.
//!
//! # The exact tier
//!
//! Q60 is not enough to round the sine correctly at 32-bit output. Deciding which
//! way a value lands needs it known to better than the closest a true sine gets to
//! a rounding boundary, and over 2^32 arguments against a 2^31 output range that
//! is around `2^-63` -- three orders of magnitude past the `2^-44` the seven-term
//! Q60 polynomial achieves.
//!
//! So [`sin_snorm`] runs two stages. The Q60 path answers first, and its result is
//! accepted unless it lands within its own proven error bound of a rounding
//! boundary, which happens for about one 32-bit phase in 256. Those few recompute
//! in [`sin_q_wide`], a Q100 mirror of the same algorithm whose error sits near
//! `2^-87` -- some 24 bits of margin over the closest approach. Q100 needs 200-bit
//! products, which is what [`mul_shift`] is for; there is no 256-bit integer type
//! to lean on.
//!
//! None of this touches the 8- and 16-bit outputs. Their last bit is coarse
//! enough that Q60 already rounds every one of their inputs correctly, and their
//! domains are small enough to prove it by walking them, so they take the fast
//! path unconditionally and cost exactly what they always did.
//!
//! # Derived constants
//!
//! Pi is not written down here. It is evaluated at compile time from Machin's
//! formula
//!
//! ```text
//! pi = 16 * atan(1/5) - 4 * atan(1/239)
//! ```
//!
//! using [`atan_series`], the Gregory series for arctangent. Both arguments are
//! small enough that the series converges in well under twenty terms. The
//! polynomial coefficients and the CORDIC table of `atan(2^-i)` come from the
//! same place. Nothing depends on a hand-transcribed digit string, and the tests
//! at the bottom of this file check every derived value against `f64` and against
//! Euler's independent identity for pi.

#![allow(
    clippy::redundant_pub_crate,
    reason = "the workspace enables unreachable_pub, which wants the opposite of what this nursery lint suggests for a private module's items"
)]
mod arc;
mod sine;
#[cfg(test)]
mod tests;
mod wide;

pub(crate) use arc::{asin_bits, atan2_bits, atan2_fast_bits};
pub(crate) use sine::{cos_snorm, sin_fast_q30, sin_snorm, tan_i24f8};

/// Fractional bits in the internal representation.
const Q: u32 = 60;

/// `1.0` in the internal representation.
const ONE: i64 = 1 << Q;

/// Multiplies two Q60 values, truncating toward negative infinity.
///
/// The widening to `i128` is for the product alone. Both operands stay `i64`, so
/// this is a 64x64-to-128 multiply and a shift, not `i128` arithmetic.
const fn mulq(a: i64, b: i64) -> i64 {
    (((a as i128) * (b as i128)) >> Q) as i64
}

/// Divides, rounding halfway cases away from zero.
///
/// `d` must be non-zero, and `2 * n` must not overflow.
const fn div_round(n: i64, d: i64) -> i64 {
    let (n, d) = if d < 0 { (-n, -d) } else { (n, d) };
    if n >= 0 {
        (2 * n + d) / (2 * d)
    } else {
        -((-2 * n + d) / (2 * d))
    }
}

/// Evaluates `atan(z)` in Q60 by the Gregory series.
///
/// `z` is a Q60 value and must satisfy `|z| < 1`, since the series diverges at
/// unity. Terms shrink by a factor of `z^2` each step, so the loop ends once the
/// running power underflows the last bit. Only ever called at compile time.
const fn atan_series(z: i64) -> i64 {
    let z2 = mulq(z, z);
    let mut power = z;
    let mut k: i64 = 0;
    let mut acc: i64 = 0;
    while power != 0 {
        let term = power / (2 * k + 1);
        if k % 2 == 0 {
            acc += term;
        } else {
            acc -= term;
        }
        power = mulq(power, z2);
        k += 1;
    }
    acc
}

/// Pi in Q60, from Machin's formula.
const PI: i64 = 16 * atan_series(ONE / 5) - 4 * atan_series(ONE / 239);

/// A full turn in Q60 radians.
const TWO_PI: i64 = 2 * PI;

/// One turn per `2*pi` radians, in Q60.
///
/// Multiplying by this converts radians to turns without a division.
const TURNS_PER_RADIAN: i64 = (((ONE as i128) << Q) / (TWO_PI as i128)) as i64;
/// Scales a Q30 value in `[-1, 1]` to a signed-normalized bit pattern, in 32
/// bits.
///
/// The counterpart of [`q_to_snorm`] for the approximate tier, and 32-bit clean
/// for the same reason [`sin_fast_q30`] is. `max` is the bit pattern denoting
/// `1.0`, always `2^(bits-1) - 1`, and `bits` is the width of the output type.
///
/// The obvious `v * max >> 30` is a 61-bit product, which is exactly what is not
/// available here, so the scale is split by width. Up to sixteen bits the value
/// is halved to Q15 first, leaving a product of two 16-bit numbers. At 32 bits
/// `max` is `2^31 - 1`, so the answer is `2v - v/2^30` and the second term is
/// zero or one -- evaluated in `u32`, where `2v` still has somewhere to live.
pub(crate) const fn q30_to_snorm(v: i32, max: i32, bits: u32) -> i32 {
    let magnitude = v.unsigned_abs();
    let scaled = if bits >= 32 {
        magnitude * 2 - if magnitude > (1 << 29) { 1 } else { 0 }
    } else {
        let halved = (magnitude + (1 << 14)) >> 15;
        (halved * (max as u32) + (1 << 14)) >> 15
    };
    // Reapply the sign by mask rather than by branch, as [`sin_fast_q30`] does.
    let sign = v >> 31;
    ((scaled as i32) ^ sign) - sign
}

/// Scales a Q60 value in `[-1, 1]` to a signed-normalized bit pattern.
///
/// `max` is the bit pattern denoting `1.0`. Halfway cases round away from zero.
pub(crate) const fn q_to_snorm(v: i64, max: i64) -> i64 {
    let half = 1_i128 << (Q - 1);
    let scaled = (v as i128) * (max as i128);
    if v >= 0 {
        ((scaled + half) >> Q) as i64
    } else {
        -(((-scaled + half) >> Q) as i64)
    }
}

/// One over a signed-normalized scale, in Q60 with 32 guard bits.
///
/// [`asin_bits`] takes this rather than computing it, because a caller can
/// evaluate it as a `const` -- its scale is always a type's `MAX` -- and a runtime
/// 128-bit division would otherwise cost more than the rest of the arcsine put
/// together.
///
/// The 32 guard bits are not decoration: the arcsine amplifies error by up to
/// four turns per unit as its argument approaches one, so a plain `ONE / max`
/// would leave `2^-29` of slack in the sine and thirty visible bits of phase near
/// the endpoints. This leaves `2^-61`.
pub(crate) const fn snorm_reciprocal(max: i64) -> i128 {
    ((ONE as i128) << 32) / (max as i128)
}
/// Converts Q60 radians to a phase with `bits` bits, as a signed offset.
///
/// The result lies in `-2^(bits-1) ..= 2^(bits-1)`, the phase read as a signed
/// offset from zero rather than as a position on `0 .. 2^bits`. Casting it to
/// the caller's storage type -- signed for a [pitch](crate::pitch), unsigned
/// for an [angle](crate::angle) -- keeps the `bits` bits that matter and
/// discards the sign extension, which is exactly the wrapping the phase space
/// wants. Staying signed all the way to that cast is what lets the pitch types
/// hold the result without a mask-then-reinterpret round trip.
pub(crate) const fn rad_to_bits(radians: i64, bits: u32) -> i32 {
    let turns = mulq(radians, TURNS_PER_RADIAN);
    let shift = Q - bits;
    let half = 1 << (shift - 1);
    let rounded = if turns >= 0 {
        (turns + half) >> shift
    } else {
        -((-turns + half) >> shift)
    };
    rounded as i32
}

/// Bits of phase in one octant of the `u32` phase space.
const OCTANT_BITS: u32 = 29;

/// One octant of the `u32` phase space.
const OCTANT: u32 = 1 << OCTANT_BITS;
