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
//! Intermediates are `i128` in Q60 — an integer `v` denotes `v / 2^60`. That
//! leaves 60 fractional bits against the 4.7e-10 resolution of
//! [`Signed32`](crate::Signed32), so rounding noise in the pipeline is roughly
//! nine orders of magnitude below the last bit of the widest output.
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
//! CORDIC table of `atan(2^-i)` comes from the same function. Nothing depends
//! on a hand-transcribed digit string, and the tests at the bottom of this file
//! check each derived value against `f64` and against Euler's independent
//! identity for pi.

#![allow(
    clippy::redundant_pub_crate,
    reason = "the workspace enables unreachable_pub, which wants the opposite of what this nursery lint suggests for a private module's items"
)]

/// Fractional bits in the internal representation.
const Q: u32 = 60;

/// `1.0` in the internal representation.
const ONE: i128 = 1 << Q;

/// Multiplies two Q60 values, truncating toward negative infinity.
const fn mulq(a: i128, b: i128) -> i128 {
    (a * b) >> Q
}

/// Divides, rounding halfway cases away from zero.
///
/// `d` must be non-zero, and `2 * n` must not overflow.
const fn div_round(n: i128, d: i128) -> i128 {
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
/// unity. Terms shrink by a factor of `z^2` each step, so the loop ends once
/// the running power underflows the last bit.
const fn atan_series(z: i128) -> i128 {
    let z2 = mulq(z, z);
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
        power = mulq(power, z2);
        k += 1;
    }
    acc
}

/// Pi in Q60, from Machin's formula.
const PI: i128 = 16 * atan_series(ONE / 5) - 4 * atan_series(ONE / 239);

/// A full turn in Q60 radians.
const TWO_PI: i128 = 2 * PI;

/// Number of CORDIC rotations performed by [`atan2_bits`].
///
/// The residual angle after `n` rotations is bounded by `atan(2^-n)`, so 40
/// rotations leave under 1e-12 radians of error — three orders of magnitude
/// finer than the 1.5e-9 radian last bit of [`Angle32`](crate::Angle32).
const CORDIC_ITERS: usize = 40;

/// Scale that CORDIC coordinates are normalized to before rotating.
///
/// The rotations grow the vector by the CORDIC gain (about 1.647) and shift
/// coordinates right by up to `CORDIC_ITERS` bits, so the working scale needs
/// headroom above and precision below. 100 bits leaves 27 bits of margin
/// against `i128` overflow and 60 bits below the smallest shift.
const CORDIC_SCALE_BITS: u32 = 100;

/// `atan(2^-i)` in Q60 radians for each CORDIC rotation.
const ATAN_POW2: [i128; CORDIC_ITERS] = {
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

/// Evaluates `sin(x)` in Q60 for `0 <= x <= pi/4`.
///
/// Seven terms of the Taylor series. The first omitted term is `x^15 / 15!`,
/// which peaks at 3.7e-14 over this interval.
const fn sin_poly(x: i128) -> i128 {
    let x2 = mulq(x, x);
    let mut t = ONE - x2 / 156;
    t = ONE - mulq(x2, t) / 110;
    t = ONE - mulq(x2, t) / 72;
    t = ONE - mulq(x2, t) / 42;
    t = ONE - mulq(x2, t) / 20;
    t = ONE - mulq(x2, t) / 6;
    mulq(x, t)
}

/// Evaluates `cos(x)` in Q60 for `0 <= x <= pi/4`.
///
/// Seven terms of the Taylor series. The first omitted term is `x^14 / 14!`,
/// which peaks at 4.3e-13 over this interval.
const fn cos_poly(x: i128) -> i128 {
    let x2 = mulq(x, x);
    let mut t = ONE - x2 / 132;
    t = ONE - mulq(x2, t) / 90;
    t = ONE - mulq(x2, t) / 56;
    t = ONE - mulq(x2, t) / 30;
    t = ONE - mulq(x2, t) / 12;
    ONE - mulq(x2, t) / 2
}

/// Bits of phase in one octant of the `u32` phase space.
const OCTANT_BITS: u32 = 29;

/// One octant of the `u32` phase space.
const OCTANT: u32 = 1 << OCTANT_BITS;

/// Returns `sin(2*pi * phase / 2^32)` in Q60.
///
/// The phase is folded into the first octant using the eightfold symmetry of
/// the sine, which bounds the polynomial argument by `pi/4` where the Taylor
/// series is at its most accurate. The results at multiples of a quarter turn
/// are exactly `-1`, `0`, and `1`.
pub(crate) const fn sin_q(phase: u32) -> i128 {
    let octant = phase >> OCTANT_BITS;
    let offset = phase & (OCTANT - 1);

    // Odd octants run backwards: measure from the far end instead.
    let folded = if octant % 2 == 1 {
        OCTANT - offset
    } else {
        offset
    };
    let x = ((folded as i128) * TWO_PI) >> 32;

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
pub(crate) const fn cos_q(phase: u32) -> i128 {
    sin_q(phase.wrapping_add(1 << 30))
}

/// Returns `sin(2*pi * phase / 2^32)` in Q60 using a cheap approximation.
///
/// A parabola through the sine's zeros and peak, corrected by a second
/// parabola in its own output — the classic
/// `0.775 * y + 0.225 * y * |y|` refinement. Worst-case error is 1.1e-3, and
/// the values at multiples of a quarter turn are still exact.
pub(crate) const fn sin_fast_q(phase: u32) -> i128 {
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
    let refined =
        (parabola * W_LINEAR + ((parabola * parabola.abs()) >> P) * W_SQUARE) >> P;

    (refined as i128) << (Q - P)
}

/// Scales a Q60 value in `[-1, 1]` to a signed-normalized bit pattern.
///
/// `max` is the bit pattern denoting `1.0`. Halfway cases round away from zero.
pub(crate) const fn q_to_snorm(v: i128, max: i128) -> i128 {
    let half = 1 << (Q - 1);
    if v >= 0 {
        (v * max + half) >> Q
    } else {
        -((-v * max + half) >> Q)
    }
}

/// Returns `tan(2*pi * phase / 2^32)` as an [`I24F8`](crate::I24F8) bit pattern.
///
/// Saturates at the poles, where the cosine is exactly zero.
pub(crate) const fn tan_i24f8(phase: u32) -> i32 {
    let sin = sin_q(phase);
    let cos = cos_q(phase);
    if cos == 0 {
        return if sin >= 0 { i32::MAX } else { i32::MIN };
    }
    let scaled = div_round(sin * 256, cos);
    if scaled > i32::MAX as i128 {
        i32::MAX
    } else if scaled < i32::MIN as i128 {
        i32::MIN
    } else {
        scaled as i32
    }
}

/// Converts Q60 radians to a phase with `bits` bits, wrapping into range.
pub(crate) const fn rad_to_bits(radians: i128, bits: u32) -> u32 {
    let turns = div_round(radians * (1 << bits), TWO_PI);
    (turns as u32) & (u32::MAX >> (32 - bits))
}

/// Returns the angle of `(x, y)` in Q60 radians, in the range `(-pi, pi]`.
///
/// CORDIC vectoring: the vector is rotated by successively smaller arctangents
/// chosen to drive `y` toward zero, and the rotations that were applied sum to
/// the original angle. Every rotation is a shift and an add, so no division or
/// multiplication appears in the loop.
const fn atan2_q(y: i64, x: i64) -> i128 {
    if x == 0 && y == 0 {
        return 0;
    }

    let mut px = x as i128;
    let mut py = y as i128;

    // CORDIC only converges over the right half plane. Rotating a left-half
    // point by a half turn moves it there, at the cost of a half turn in the
    // result — added back with the sign that keeps the answer in (-pi, pi].
    let base = if px < 0 {
        let half = if py >= 0 { PI } else { -PI };
        px = -px;
        py = -py;
        half
    } else {
        0
    };

    // Normalize upward so the deepest rotation still has bits to shift.
    let ax = px.unsigned_abs();
    let ay = py.unsigned_abs();
    let magnitude = if ax > ay { ax } else { ay };
    let shift = CORDIC_SCALE_BITS as i32 - (128 - magnitude.leading_zeros()) as i32;
    if shift >= 0 {
        px <<= shift;
        py <<= shift;
    } else {
        px >>= -shift;
        py >>= -shift;
    }

    let mut angle = 0;
    let mut i = 0;
    while i < CORDIC_ITERS {
        if py > 0 {
            let rotated_x = px + (py >> i);
            py -= px >> i;
            px = rotated_x;
            angle += ATAN_POW2[i];
        } else if py < 0 {
            let rotated_x = px - (py >> i);
            py += px >> i;
            px = rotated_x;
            angle -= ATAN_POW2[i];
        }
        i += 1;
    }

    base + angle
}

/// Returns the angle of `(x, y)` as a phase with `bits` bits.
pub(crate) const fn atan2_bits(y: i64, x: i64, bits: u32) -> u32 {
    rad_to_bits(atan2_q(y, x), bits)
}

/// Returns the angle of `(x, y)` as a phase with `bits` bits, approximately.
///
/// Rajan's polynomial for arctangent over `[0, 1]`, applied to the smaller
/// coordinate over the larger and unfolded by octant. Worst-case error is
/// 4.4e-3 radians.
pub(crate) const fn atan2_fast_bits(y: i64, x: i64, bits: u32) -> u32 {
    /// Fractional bits used by the approximation.
    const P: u32 = 30;
    /// One turn for the approximation.
    const TURN: i64 = 1 << P;
    /// `0.273 / (2 * pi)`, the polynomial's correction weight, expressed in
    /// turns. Derived from [`TWO_PI`] by integer division rather than written
    /// down, so this file holds no floating-point constant.
    const CORRECTION: i64 =
        ((273 * (TURN as i128) << Q) / (1000 * TWO_PI)) as i64;

    if x == 0 && y == 0 {
        return 0;
    }

    let ax = (x as i128).unsigned_abs();
    let ay = (y as i128).unsigned_abs();
    let steep = ay > ax;
    let (numerator, denominator) = if steep { (ax, ay) } else { (ay, ax) };

    // The ratio of the smaller coordinate to the larger, in [0, 1].
    let ratio = ((numerator << P) / denominator) as i64;
    let mut turns =
        (ratio >> 3) + ((CORRECTION * ((ratio * (TURN - ratio)) >> P)) >> P);

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
        div_round((turns as i128) << bits, TURN as i128) as i64
    };
    (bit_turns as u32) & (u32::MAX >> (32 - bits))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::panic,
        clippy::float_cmp,
        reason = "tests assert; panicking is how a test reports failure"
    )]

    use super::{ATAN_POW2, CORDIC_ITERS, ONE, PI, Q, TWO_PI, atan_series, mulq};

    /// Converts an internal Q60 value to `f64` for comparison against `std`.
    fn to_f64(v: i128) -> f64 {
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
        assert!((PI - euler).abs() < 64, "machin and euler differ by {}", PI - euler);
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
    fn mulq_is_exact_for_powers_of_two() {
        assert_eq!(mulq(ONE, ONE), ONE);
        assert_eq!(mulq(ONE >> 1, ONE >> 1), ONE >> 2);
        assert_eq!(mulq(-(ONE >> 1), ONE >> 1), -(ONE >> 2));
        assert_eq!(mulq(0, ONE), 0);
    }

    #[test]
    fn the_approximation_weights_are_derived_correctly() {
        // These are integer-derived so that no floating-point constant appears
        // outside the conversion functions. Check them against the intent.
        const P_ONE: i64 = 1 << 30;
        assert_eq!(31 * P_ONE / 40, (0.775 * f64::from(P_ONE as u32)) as i64);
        assert_eq!(31 * P_ONE / 40 + (P_ONE - 31 * P_ONE / 40), P_ONE);

        let correction = ((273 * i128::from(P_ONE) << Q) / (1000 * TWO_PI)) as i64;
        let expected = (0.273 / core::f64::consts::TAU * f64::from(P_ONE as u32)) as i64;
        assert!(
            (correction - expected).abs() <= 1,
            "correction weight {correction} vs {expected}"
        );
    }

    #[test]
    fn q_is_the_documented_scale() {
        assert_eq!(Q, 60);
        assert_eq!(ONE, 1_i128 << 60);
    }
}
