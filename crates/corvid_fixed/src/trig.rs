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
//! The one place `i128` still earns its keep is [`asin_bits`] at 32-bit output,
//! where the square root needs a 120-bit radicand.
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
/// Seven terms. The first omitted term is `x^15 / 15!`, which peaks at 3.7e-14
/// over this interval — four orders of magnitude below the last bit of the widest
/// output type.
const fn sin_poly(x: i64) -> i64 {
    mulq(x, horner(mulq(x, x), &SIN_COEFFICIENTS))
}

/// Evaluates `cos(x)` in Q60 for `0 <= x <= pi/4`.
///
/// Seven terms. The first omitted term is `x^14 / 14!`, which peaks at 4.3e-13.
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

    use super::{
        ATAN_POW2, CORDIC_ITERS, CORDIC_SCALE_BITS, COS_COEFFICIENTS, ONE, PI, Q, SIN_COEFFICIENTS,
        TERMS, TURNS_PER_RADIAN, TWO_PI, atan_series, mulq,
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
    fn q_is_the_documented_scale() {
        assert_eq!(Q, 60);
        assert_eq!(ONE, 1_i64 << 60);
        // Q60 in an i64 tops out just under 8.0. A full turn in radians is the
        // largest value this module represents, and it fits with room to spare.
        assert_eq!(i64::MAX / ONE, 7);
        assert_eq!(TWO_PI / ONE, 6, "a turn should be just over six in Q60");
    }
}
