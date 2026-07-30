//! Integer-only trigonometry backing the [angle types](crate::Angle16).
//!
//! Everything here is `const` and uses only integer arithmetic, so results are
//! bit-identical on every target. No floating-point value is produced, consumed,
//! or even written down: every constant in this file, pi included, is derived by
//! integer arithmetic at compile time, and no lookup table is baked into the
//! binary as literal data.
//!
//! # Internal representation
//!
//! Values are `i64` in Q60 — an integer `v` denotes `v / 2^60`. That leaves 60
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
//! `cargo run --release --example bench` reproduces the comparison.
//!
//! `i128` earns its keep in two places: [`asin_bits`] at 32-bit output, where the
//! square root needs a 120-bit radicand, and the exact tier below.
//!
//! # The exact tier
//!
//! Q60 is not enough to round the sine correctly at 32-bit output. Deciding which
//! way a value lands needs it known to better than the closest a true sine gets to
//! a rounding boundary, and over 2^32 arguments against a 2^31 output range that
//! is around `2^-63` — three orders of magnitude past the `2^-44` the seven-term
//! Q60 polynomial achieves.
//!
//! So [`sin_snorm`] runs two stages. The Q60 path answers first, and its result is
//! accepted unless it lands within its own proven error bound of a rounding
//! boundary, which happens for about one 32-bit phase in 256. Those few recompute
//! in [`sin_q_wide`], a Q100 mirror of the same algorithm whose error sits near
//! `2^-87` — some 24 bits of margin over the closest approach. Q100 needs 200-bit
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

/// Number of CORDIC rotations performed by [`atan2_bits`].
///
/// The residual angle after `n` rotations is bounded by `atan(2^-n)`, so 40
/// rotations leave under 1e-12 radians of error — three orders of magnitude finer
/// than the 1.5e-9 radian last bit of [`Angle32`](crate::Angle32).
const CORDIC_ITERS: usize = 40;

/// Scale that CORDIC coordinates are normalized to before rotating.
///
/// The rotations grow the vector by the CORDIC gain, about 1.647, so the working
/// scale needs headroom above it: `2^61 * 1.647` still fits `i64`. Precision
/// below is not a concern — each rotation's truncation costs one unit against a
/// magnitude of `2^61`.
const CORDIC_SCALE_BITS: u32 = 61;

/// `atan(2^-i)` in Q60 radians for each CORDIC rotation.
const ATAN_POW2: [i64; CORDIC_ITERS] = {
    let mut table = [0; CORDIC_ITERS];
    // atan(1) is outside the series' radius of convergence, but it is pi/4.
    table[0] = PI / 4;
    let mut i = 1;
    while i < CORDIC_ITERS {
        table[i] = atan_series(ONE >> i);
        i += 1;
    }
    table
};

/// Number of terms in the sine and cosine polynomials.
const TERMS: usize = 7;

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
const SIN_COEFFICIENTS: [i64; TERMS] = taylor_coefficients(2);

/// Coefficients of `cos(x)` as a polynomial in `x^2`.
const COS_COEFFICIENTS: [i64; TERMS] = taylor_coefficients(1);

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
/// over this interval — four orders of magnitude below the last bit of the widest
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

/// Bits of phase in one octant of the `u32` phase space.
const OCTANT_BITS: u32 = 29;

/// One octant of the `u32` phase space.
const OCTANT: u32 = 1 << OCTANT_BITS;

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

/// Returns `sin(2*pi * phase / 2^32)` in Q60 using a cheap approximation.
///
/// A parabola through the sine's zeros and peak, corrected by a second parabola
/// in its own output — the classic `0.775 * y + 0.225 * y * |y|` refinement.
/// Worst-case error is 1.1e-3, and the values at multiples of a quarter turn are
/// still exact.
pub(crate) const fn sin_fast_q(phase: u32) -> i64 {
    /// Fractional bits used by the approximation.
    const P: u32 = 30;
    /// `1.0` for the approximation.
    const P_ONE: i64 = 1 << P;
    /// Weight of the linear term in the refinement, `0.775` as `31/40`.
    const W_LINEAR: i64 = 31 * P_ONE / 40;
    /// Weight of the quadratic term. The two sum to one, which is what keeps the
    /// peak of the approximated sine exactly `1.0`.
    const W_SQUARE: i64 = P_ONE - W_LINEAR;

    // Reinterpreting the phase as signed maps it to signed turns; doubling that
    // gives half-turns, the interval [-1, 1) the parabola is defined over. The
    // shift discards one bit of phase, which is 1.5e-9 of a turn against this
    // approximation's 1.1e-3 error.
    let z = (phase as i32 as i64) >> 1;
    let parabola = (z * (P_ONE - z.abs())) >> (P - 2);
    let refined = (parabola * W_LINEAR + ((parabola * parabola.abs()) >> P) * W_SQUARE) >> P;

    refined << (Q - P)
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
/// evaluate it as a `const` — its scale is always a type's `MAX` — and a runtime
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

/// Fractional bits in the extended representation used by the exact tier.
const Q_WIDE: u32 = 100;

/// `1.0` in the extended representation.
const ONE_WIDE: i128 = 1 << Q_WIDE;

/// Low 64 bits of a `u128`.
const LIMB: u128 = u64::MAX as u128;

/// Multiplies two `i128` values and shifts the full 256-bit product right by
/// `shift`, truncating toward zero.
///
/// `shift` must lie in `1 ..= 127`, and the shifted result must fit an `i128`.
///
/// Rust has no 256-bit integer, so the product is assembled from four 64-bit
/// limbs by hand. Signs are stripped first and reapplied at the end, which keeps
/// the limb arithmetic unsigned and makes the truncation symmetric about zero —
/// the same convention [`q_to_snorm`] already rounds under.
const fn mul_shift(a: i128, b: i128, shift: u32) -> i128 {
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
const PI_WIDE: i128 = 16 * atan_series_wide(ONE_WIDE / 5) - 4 * atan_series_wide(ONE_WIDE / 239);

/// A full turn in Q100 radians.
const TWO_PI_WIDE: i128 = 2 * PI_WIDE;

/// Number of terms in the extended-precision sine and cosine polynomials.
///
/// Twelve leaves the first omitted term at `x^25 / 25!` for the sine and
/// `x^24 / 24!` for the cosine, which peak at 1.5e-28 and 4.9e-27 over the first
/// octant. The cosine is the weaker of the two and sets the tier's accuracy.
const TERMS_WIDE: usize = 12;

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
const SIN_COEFFICIENTS_WIDE: [i128; TERMS_WIDE] = taylor_coefficients_wide(2);

/// Q100 coefficients of `cos(x)` as a polynomial in `x^2`.
const COS_COEFFICIENTS_WIDE: [i128; TERMS_WIDE] = taylor_coefficients_wide(1);

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
const fn sin_q_wide(phase: u32) -> i128 {
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
/// bit — far below the polynomial's own error, and far below the closest a true
/// sine comes to a rounding boundary.
const WIDE_GUARD: u32 = 60;

/// Scales a Q100 value in `[-1, 1]` to a signed-normalized bit pattern.
///
/// `max` is the bit pattern denoting `1.0`. Halfway cases round away from zero,
/// matching [`q_to_snorm`].
const fn q_to_snorm_wide(v: i128, max: i64) -> i64 {
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
/// peaks at 3.9e-13 over the first octant — about `2^18.8` here. The rest is
/// small change: seven truncations inside [`horner`], one in the argument
/// reduction, and the error carried by [`TWO_PI`] itself, together under thirty
/// units. Rounding up to `2^20` leaves better than a factor of two in hand.
const SIN_Q_ERROR: i64 = 1 << 20;

/// Returns `sin(2*pi * phase / 2^32)` as a signed-normalized bit pattern,
/// correctly rounded.
///
/// Only the 32-bit output does any work here. Its Q60 answer stands unless it
/// falls within [`SIN_Q_ERROR`] of a rounding boundary, where it cannot be
/// trusted to land on the right side; those recompute in Q100. The window covers
/// `2^-8` of the phase space, so about one call in 256 takes the slow path, which
/// costs the 32-bit sine roughly a tenth of its time overall. The narrower
/// outputs skip the test entirely — see the comment below.
///
/// Inlining is not optional here. `max` is a constant at every call site, and the
/// width test turns into nothing at all once it is one; left out of line, the
/// narrow types pay for a decision that was made when their type was chosen.
#[inline]
pub(crate) const fn sin_snorm(phase: u32, max: i64) -> i64 {
    let v = sin_q(phase);

    // Outputs of sixteen bits or fewer never need the exact tier. The Q60 error
    // is 2^-44, nine orders of magnitude below their last bit, and their whole
    // domain is small enough to prove it rather than argue it — which
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
    // inside i64 — `sin << 8` would not fit. The shift leaves the divisor 52
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

/// Converts Q60 radians to a phase with `bits` bits, as a signed offset.
///
/// The result lies in `-2^(bits-1) ..= 2^(bits-1)`, the phase read as a signed
/// offset from zero rather than as a position on `0 .. 2^bits`. Casting it to
/// the caller's storage type — signed for a [pitch](crate::Pitch16), unsigned
/// for an [angle](crate::Angle16) — keeps the `bits` bits that matter and
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

/// Rotations needed for a phase of `bits` bits.
///
/// The residual after `n` rotations is `atan(2^-n)`, just under `2^-n` radians,
/// or `2^-n / (2*pi)` of a turn. Matching that to a quarter of the last bit of
/// the output needs only `bits` rotations; the eight extra put the residual
/// two hundred times below where it could disturb a rounding.
const fn cordic_iters(bits: u32) -> usize {
    let wanted = bits as usize + 8;
    if wanted < CORDIC_ITERS {
        wanted
    } else {
        CORDIC_ITERS
    }
}

/// Returns the angle of `(x, y)` in Q60 radians, in the range `(-pi, pi]`.
///
/// CORDIC vectoring: the vector is rotated by successively smaller arctangents
/// chosen to drive `y` toward zero, and the rotations that were applied sum to
/// the original angle. Every rotation is a shift and an add, so no division or
/// multiplication appears in the loop.
///
/// The direction of each rotation depends on the sign of `y`, which is exactly
/// the kind of branch a processor cannot predict. Deriving a `+1`/`-1` multiplier
/// from the sign bit instead makes the loop straight-line code, which measured
/// three times faster than the equivalent `if`.
const fn atan2_q(y: i64, x: i64, iters: usize) -> i64 {
    if x == 0 && y == 0 {
        return 0;
    }

    // Normalize first, and in i128, so that the deepest rotation still has bits
    // to shift and so `i64::MIN` cannot trap the negation below.
    let magnitude = {
        let ax = x.unsigned_abs();
        let ay = y.unsigned_abs();
        if ax > ay { ax } else { ay }
    };
    let shift = CORDIC_SCALE_BITS as i32 - (64 - magnitude.leading_zeros()) as i32;
    let (mut px, mut py) = if shift >= 0 {
        (
            (((x as i128) << shift) as i64),
            (((y as i128) << shift) as i64),
        )
    } else {
        (x >> -shift, y >> -shift)
    };

    // CORDIC only converges over the right half plane. Rotating a left-half point
    // by a half turn moves it there, at the cost of a half turn in the result —
    // added back with the sign that keeps the answer in (-pi, pi].
    let base = if px < 0 {
        let half = if py >= 0 { PI } else { -PI };
        px = -px;
        py = -py;
        half
    } else {
        0
    };

    let mut angle = 0;
    let mut i = 0;
    while i < iters {
        // -1 when py is negative, +1 otherwise. Rotating the wrong way on a py of
        // exactly zero costs nothing: the next rotation turns straight back.
        let direction = (py >> 63) | 1;
        let rotated_x = px + direction * (py >> i);
        py -= direction * (px >> i);
        px = rotated_x;
        angle += direction * ATAN_POW2[i];
        i += 1;
    }

    base + angle
}

/// Returns the angle of `(x, y)` as a signed phase with `bits` bits.
///
/// See [`rad_to_bits`] for the sign convention.
pub(crate) const fn atan2_bits(y: i64, x: i64, bits: u32) -> i32 {
    rad_to_bits(atan2_q(y, x, cordic_iters(bits)), bits)
}

/// Returns `asin(value / max)` as a signed phase with `bits` bits, within a
/// quarter turn of zero.
///
/// Uses `asin(v) = atan2(v, sqrt(1 - v^2))`, so [`atan2_bits`]'s accuracy carries
/// over. `max` must be positive, `|value|` must not exceed it, and `reciprocal`
/// must be [`snorm_reciprocal(max)`](snorm_reciprocal).
///
/// The result is signed, so it drops straight into a [pitch](crate::Pitch16) —
/// whose range is exactly the arcsine's — without a wrapping reinterpretation.
pub(crate) const fn asin_bits(value: i64, max: i64, reciprocal: i128, bits: u32) -> i32 {
    let quarter = 1_i32 << (bits - 2);

    // The endpoints are exact by definition rather than by approximation: at
    // plus or minus one the cosine is zero, and the arctangent would be looking
    // at nothing over nothing.
    if value >= max {
        return quarter;
    }
    if value <= -max {
        return -quarter;
    }

    let sine = (((value as i128) * reciprocal) >> 32) as i64;
    let cosine = if bits <= 16 {
        // Q30 leaves fourteen bits of headroom over a 16-bit phase, and a 64-bit
        // square root is far cheaper than a 128-bit one.
        (((ONE - mulq(sine, sine)) as u64).isqrt() as i64) << (Q / 2)
    } else {
        // A Q60 cosine needs a 120-bit radicand: the one place in this module
        // where i128 arithmetic pays for itself.
        let one = ONE as u128;
        let magnitude = sine.unsigned_abs() as u128;
        (one * one - magnitude * magnitude).isqrt() as i64
    };

    atan2_bits(sine, cosine, bits)
}

/// Returns the angle of `(x, y)` as a signed phase with `bits` bits,
/// approximately.
///
/// Rajan's polynomial for arctangent over `[0, 1]`, applied to the smaller
/// coordinate over the larger and unfolded by octant. Worst-case error is 4.4e-3
/// radians. See [`rad_to_bits`] for the sign convention.
pub(crate) const fn atan2_fast_bits(y: i64, x: i64, bits: u32) -> i32 {
    /// Fractional bits used by the approximation.
    const P: u32 = 30;
    /// One turn for the approximation.
    const TURN: i64 = 1 << P;
    /// `0.273 / (2 * pi)`, the polynomial's correction weight, expressed in
    /// turns. Derived from [`TWO_PI`] by integer division rather than written
    /// down, so this file holds no floating-point constant.
    const CORRECTION: i64 = (((273 * (TURN as i128)) << Q) / (1000 * TWO_PI as i128)) as i64;

    if x == 0 && y == 0 {
        return 0;
    }

    let ax = x.unsigned_abs();
    let ay = y.unsigned_abs();
    let steep = ay > ax;
    let (numerator, denominator) = if steep { (ax, ay) } else { (ay, ax) };

    // Shifting both coordinates down to 34 bits keeps `numerator << P` inside a
    // u64, so the ratio costs one 64-bit division rather than a 128-bit one. The
    // 34 bits that survive are ten orders of magnitude finer than this
    // approximation's own error.
    let excess = (64 - denominator.leading_zeros()).saturating_sub(34);
    let ratio = (((numerator >> excess) << P) / (denominator >> excess)) as i64;
    let mut turns = (ratio >> 3) + ((CORRECTION * ((ratio * (TURN - ratio)) >> P)) >> P);

    // Unfold the octant, then the quadrant.
    if steep {
        turns = TURN / 4 - turns;
    }
    if x < 0 {
        turns = TURN / 2 - turns;
    }
    if y < 0 {
        turns = -turns;
    }

    let bit_turns = if bits >= P {
        turns << (bits - P)
    } else {
        div_round(turns << bits, TURN)
    };
    bit_turns as i32
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::panic,
        clippy::float_cmp,
        reason = "tests assert; panicking is how a test reports failure"
    )]

    extern crate std;

    use super::{
        ATAN_POW2, CORDIC_ITERS, CORDIC_SCALE_BITS, COS_COEFFICIENTS, COS_COEFFICIENTS_WIDE, ONE,
        ONE_WIDE, PI, PI_WIDE, Q, Q_WIDE, SIN_COEFFICIENTS, SIN_COEFFICIENTS_WIDE, SIN_Q_ERROR,
        TERMS, TERMS_WIDE, TURNS_PER_RADIAN, TWO_PI, atan_series, mul_shift, mulq, q_to_snorm_wide,
        sin_q_wide, sin_snorm,
    };

    /// Converts an internal Q60 value to `f64` for comparison against `std`.
    fn to_f64(v: i64) -> f64 {
        v as f64 / (ONE as f64)
    }

    #[test]
    fn pi_matches_std() {
        let error = (to_f64(PI) - core::f64::consts::PI).abs();
        assert!(error < 1e-17, "pi off by {error:e}");
        assert_eq!(TWO_PI, 2 * PI);
    }

    #[test]
    fn machin_agrees_with_a_different_identity() {
        // Euler: pi/4 = atan(1/2) + atan(1/3). An independent route to the same
        // constant catches a transcription error in either formula.
        let euler = 4 * (atan_series(ONE / 2) + atan_series(ONE / 3));
        assert!(
            (PI - euler).abs() < 64,
            "machin and euler differ by {}",
            PI - euler
        );
    }

    #[test]
    fn turns_per_radian_is_the_reciprocal_of_a_turn() {
        let expected = 1.0 / core::f64::consts::TAU;
        assert!((to_f64(TURNS_PER_RADIAN) - expected).abs() < 1e-17);
    }

    #[test]
    fn atan_table_matches_std() {
        for (i, &entry) in ATAN_POW2.iter().enumerate() {
            let expected = (2.0_f64).powi(-(i as i32)).atan();
            let error = (to_f64(entry) - expected).abs();
            assert!(error < 1e-16, "atan(2^-{i}) off by {error:e}");
        }
    }

    #[test]
    fn atan_table_shrinks_monotonically() {
        for pair in ATAN_POW2.windows(2) {
            assert!(pair[1] < pair[0]);
        }
        assert!(ATAN_POW2[CORDIC_ITERS - 1] > 0, "table underflowed to zero");
    }

    #[test]
    fn last_rotation_bounds_the_residual_angle() {
        // The unmodelled residual after the final rotation must sit well below
        // the last bit of Angle32, which is TWO_PI / 2^32 radians.
        let last_bit = to_f64(TWO_PI) / f64::from(u32::MAX);
        assert!(to_f64(ATAN_POW2[CORDIC_ITERS - 1]) < last_bit / 100.0);
    }

    #[test]
    fn cordic_coordinates_cannot_overflow() {
        // The rotations multiply the vector length by the CORDIC gain, so the
        // working scale plus that growth has to stay inside i64.
        let gain: f64 = (0..CORDIC_ITERS)
            .map(|i| (1.0 + 4.0_f64.powi(-(i as i32))).sqrt())
            .product();
        assert!(gain < 1.65, "gain grew to {gain}");
        let peak = 2.0_f64.powi(CORDIC_SCALE_BITS as i32) * gain;
        assert!(peak < i64::MAX as f64, "peak {peak:e} exceeds i64");
    }

    #[test]
    fn taylor_coefficients_are_reciprocal_factorials() {
        // Sine's coefficients are 1/(2k+1)! with alternating signs, cosine's are
        // 1/(2k)!. Checking against f64 catches an off-by-one in the generator.
        let mut factorial = 1.0_f64;
        for (k, &coefficient) in SIN_COEFFICIENTS.iter().enumerate() {
            if k > 0 {
                factorial *= f64::from(2 * k as u32) * f64::from(2 * k as u32 + 1);
            }
            let expected = if k % 2 == 1 {
                -1.0 / factorial
            } else {
                1.0 / factorial
            };
            let error = (to_f64(coefficient) - expected).abs();
            assert!(error < 1e-17, "sin coefficient {k} off by {error:e}");
        }

        let mut factorial = 1.0_f64;
        for (k, &coefficient) in COS_COEFFICIENTS.iter().enumerate() {
            if k > 0 {
                factorial *= f64::from(2 * k as u32 - 1) * f64::from(2 * k as u32);
            }
            let expected = if k % 2 == 1 {
                -1.0 / factorial
            } else {
                1.0 / factorial
            };
            let error = (to_f64(coefficient) - expected).abs();
            assert!(error < 1e-17, "cos coefficient {k} off by {error:e}");
        }

        assert_eq!(
            SIN_COEFFICIENTS[0], ONE,
            "the leading term must be exactly one"
        );
        assert_eq!(
            COS_COEFFICIENTS[0], ONE,
            "the leading term must be exactly one"
        );
        assert_eq!(TERMS, 7);
    }

    #[test]
    fn the_approximation_weights_are_derived_correctly() {
        // These are integer-derived so that no floating-point constant appears
        // outside the conversion functions. Check them against the intent.
        const P_ONE: i64 = 1 << 30;
        assert_eq!(31 * P_ONE / 40, (0.775 * f64::from(P_ONE as u32)) as i64);
        assert_eq!(31 * P_ONE / 40 + (P_ONE - 31 * P_ONE / 40), P_ONE);

        let correction = (((273 * i128::from(P_ONE)) << Q) / (1000 * i128::from(TWO_PI))) as i64;
        let expected = (0.273 / core::f64::consts::TAU * f64::from(P_ONE as u32)) as i64;
        assert!(
            (correction - expected).abs() <= 1,
            "correction weight {correction} vs {expected}"
        );
    }

    #[test]
    fn mulq_is_exact_for_powers_of_two() {
        assert_eq!(mulq(ONE, ONE), ONE);
        assert_eq!(mulq(ONE >> 1, ONE >> 1), ONE >> 2);
        assert_eq!(mulq(-(ONE >> 1), ONE >> 1), -(ONE >> 2));
        assert_eq!(mulq(0, ONE), 0);
    }

    #[test]
    fn mul_shift_agrees_with_a_plain_product() {
        // Where the product fits an i128 on its own, the limb assembly has a
        // reference to be checked against.
        let cases: [i128; 8] = [0, 1, -1, 3, -7, 1 << 40, -(1 << 51), (1 << 62) - 1];
        for a in cases {
            for b in cases {
                for shift in [1_u32, 17, 63, 64, 65, 100] {
                    let product = a * b;
                    // A plain shift floors; mul_shift truncates toward zero.
                    let expected = if product < 0 {
                        -((-product) >> shift)
                    } else {
                        product >> shift
                    };
                    assert_eq!(
                        mul_shift(a, b, shift),
                        expected,
                        "mul_shift({a}, {b}, {shift})"
                    );
                }
            }
        }
    }

    #[test]
    fn mul_shift_carries_across_all_four_limbs() {
        // Operands with every limb populated exercise the carry out of the middle
        // column, which a product that fits an i128 never reaches. Multiplying by
        // a power of two is the one such case with an answer that can be written
        // down independently: the product is just a shift.
        let a = (1_i128 << 100) | (1 << 64) | (1 << 63) | 1;
        assert_eq!(mul_shift(a, 1 << 70, Q_WIDE), a >> 30);
        assert_eq!(mul_shift(-a, 1 << 70, Q_WIDE), -(a >> 30));
        assert_eq!(mul_shift(a, 1 << 64, Q_WIDE), a >> 36);

        let b = (1_i128 << 26) | (1 << 64) | 0x3039;
        assert_eq!(mul_shift(a, b, Q_WIDE), mul_shift(b, a, Q_WIDE));

        assert_eq!(mul_shift(ONE_WIDE, ONE_WIDE, Q_WIDE), ONE_WIDE);
        assert_eq!(mul_shift(ONE_WIDE, -ONE_WIDE, Q_WIDE), -ONE_WIDE);
        assert_eq!(
            mul_shift(ONE_WIDE >> 1, ONE_WIDE >> 1, Q_WIDE),
            ONE_WIDE >> 2
        );
    }

    #[test]
    fn the_wide_constants_agree_with_the_narrow_ones() {
        // Two independent evaluations of Machin's formula, at scales 40 bits
        // apart. Truncating the wide one to Q60 should land on the narrow one.
        let narrowed = (PI_WIDE >> (Q_WIDE - Q)) as i64;
        assert!(
            (narrowed - PI).abs() < 64,
            "wide and narrow pi differ by {}",
            narrowed - PI
        );

        for i in 0..TERMS {
            let sine = (SIN_COEFFICIENTS_WIDE[i] >> (Q_WIDE - Q)) as i64;
            let cosine = (COS_COEFFICIENTS_WIDE[i] >> (Q_WIDE - Q)) as i64;
            assert!(
                (sine - SIN_COEFFICIENTS[i]).abs() <= 1,
                "sin coefficient {i} disagrees"
            );
            assert!(
                (cosine - COS_COEFFICIENTS[i]).abs() <= 1,
                "cos coefficient {i} disagrees"
            );
        }
        assert_eq!(TERMS_WIDE, 12);
        assert_eq!(Q_WIDE, 100);
    }

    /// The `Signed32` scale, the only width where the exact tier does any work.
    const SNORM32: i64 = i32::MAX as i64;

    #[test]
    fn the_fast_path_error_bound_covers_what_it_claims() {
        // SIN_Q_ERROR is asserted, not measured, so measure it: over a sweep of
        // phases the Q60 result must never stray further from the Q100 one than
        // the bound allows.
        let mut state = 0x243f_6a88_85a3_08d3_u64;
        let mut worst = 0_i64;
        for _ in 0..200_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let phase = (state >> 32) as u32;
            let narrow = super::sin_q(phase);
            let wide = (sin_q_wide(phase) >> (Q_WIDE - Q)) as i64;
            let error = (narrow - wide).abs();
            if error > worst {
                worst = error;
            }
        }
        assert!(
            worst < SIN_Q_ERROR,
            "measured error {worst} against a bound of {SIN_Q_ERROR}"
        );
        assert!(
            worst > SIN_Q_ERROR / 8,
            "measured error {worst} is far under the bound of {SIN_Q_ERROR}; \
             the bound has gone stale and the exact tier is doing needless work"
        );
    }

    #[test]
    fn the_two_stages_agree_on_a_sample() {
        // The cheap version of the exhaustive test below. Biased toward the
        // phases that matter: every one of these lands inside the fallback
        // window, where the two stages have the most opportunity to disagree.
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut fallbacks = 0_u32;
        for _ in 0..400_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let phase = (state >> 32) as u32;
            let expected = q_to_snorm_wide(sin_q_wide(phase), SNORM32);
            assert_eq!(sin_snorm(phase, SNORM32), expected, "phase {phase}");

            let scaled = (super::sin_q(phase) as i128) * (SNORM32 as i128);
            let magnitude = if scaled < 0 { -scaled } else { scaled };
            let residue = (magnitude + (1_i128 << (Q - 1))) & ((1_i128 << Q) - 1);
            let bound = (SIN_Q_ERROR as i128) * (SNORM32 as i128);
            if residue < bound || residue > (1_i128 << Q) - bound {
                fallbacks += 1;
            }
        }
        // One phase in 256, by construction. Well outside that band means the
        // dispatch has drifted from what its documentation promises.
        assert!(
            (600..=2600).contains(&fallbacks),
            "{fallbacks} fallbacks in 400000 phases; expected about 1560"
        );
    }

    /// Discharges the assumption behind the width gate in [`sin_snorm`].
    ///
    /// That gate hands the 8- and 16-bit outputs straight to the Q60 path,
    /// skipping the boundary test entirely. It is allowed to because their whole
    /// domains fit in a test: over every phase either type can hold, Q60 already
    /// lands on the bit pattern Q100 does. If that ever stops being true, the
    /// gate has to go, and this is what will say so.
    #[test]
    fn the_narrow_widths_never_need_the_exact_tier() {
        for bits in 0..=u8::MAX {
            let phase = u32::from(bits) << 24;
            let max = i8::MAX as i64;
            assert_eq!(
                sin_snorm(phase, max),
                q_to_snorm_wide(sin_q_wide(phase), max),
                "Angle8 phase {phase}"
            );
        }
        for bits in 0..=u16::MAX {
            let phase = u32::from(bits) << 16;
            let max = i16::MAX as i64;
            assert_eq!(
                sin_snorm(phase, max),
                q_to_snorm_wide(sin_q_wide(phase), max),
                "Angle16 phase {phase}"
            );
        }
    }

    /// Walks every one of the 2^32 `Angle32` phases, asserting that what ships
    /// equals what the Q100 path says.
    ///
    /// This is one half of the correct-rounding argument. It shows the two-stage
    /// dispatch never lets the fast path answer where the fast path is not
    /// trustworthy — that the shipped sine is, everywhere, the rounding of the
    /// Q100 value. The other half is `tests/trig.rs`'s `EXACT` table, which pins
    /// the Q100 value itself against 80-digit arithmetic at the hardest phases
    /// the search could find. Cosine needs no separate pass: it is the sine a
    /// quarter turn along, and this covers every phase.
    ///
    /// Ignored because it takes about a minute and a half on eight cores. Run it
    /// with:
    ///
    /// ```text
    /// cargo test -p corvid_fixed --release exhaustive -- --ignored
    /// ```
    #[test]
    #[ignore = "walks all 2^32 phases; run explicitly"]
    fn sin_snorm_is_exhaustively_correctly_rounded_for_angle32() {
        let threads = std::thread::available_parallelism().map_or(4, core::num::NonZero::get);
        let span = (1_u64 << 32) / threads as u64;

        std::thread::scope(|scope| {
            for slot in 0..threads {
                scope.spawn(move || {
                    let start = slot as u64 * span;
                    let end = if slot + 1 == threads {
                        1_u64 << 32
                    } else {
                        start + span
                    };
                    for phase in start..end {
                        let phase = phase as u32;
                        let expected = q_to_snorm_wide(sin_q_wide(phase), SNORM32);
                        assert_eq!(sin_snorm(phase, SNORM32), expected, "phase {phase}");
                    }
                });
            }
        });
    }

    #[test]
    fn q_is_the_documented_scale() {
        assert_eq!(Q, 60);
        assert_eq!(ONE, 1_i64 << 60);
        // Q60 in an i64 tops out just under 8.0. A full turn in radians is the
        // largest value this module represents, and it fits with room to spare.
        assert_eq!(i64::MAX / ONE, 7);
        assert_eq!(TWO_PI / ONE, 6, "a turn should be just over six in Q60");
    }
}
