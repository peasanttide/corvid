//! The arc functions: CORDIC for the arctangent, and the arcsine built on it.

#![allow(
    clippy::redundant_pub_crate,
    reason = "the workspace enables unreachable_pub, which wants the opposite of what this nursery lint suggests for a private module's items"
)]

use super::{ONE, PI, Q, TWO_PI, atan_series, mulq, rad_to_bits};

/// Number of CORDIC rotations performed by [`atan2_bits`].
///
/// The residual angle after `n` rotations is bounded by `atan(2^-n)`, so 40
/// rotations leave under 1e-12 radians of error -- three orders of magnitude finer
/// than the 1.5e-9 radian last bit of [`Angle32`](crate::Angle32).
pub(super) const CORDIC_ITERS: usize = 40;

/// Scale that CORDIC coordinates are normalized to before rotating.
///
/// Normalization scales the larger of `|x|` and `|y|` to just under `2^61`, so
/// the vector magnitude reaches `2^61 * sqrt(2)` on the diagonal. The rotations
/// then grow it by the CORDIC gain, about 1.647, and the working scale needs
/// headroom above both: `2^61 * sqrt(2) * 1.647` still fits `i64`. Precision
/// below is not a concern -- each rotation's truncation costs one unit against a
/// magnitude of `2^61`.
pub(super) const CORDIC_SCALE_BITS: u32 = 61;

/// `atan(2^-i)` in Q60 radians for each CORDIC rotation.
pub(super) const ATAN_POW2: [i64; CORDIC_ITERS] = {
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
    // by a half turn moves it there, at the cost of a half turn in the result --
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
/// The result is signed, so it drops straight into a [pitch](crate::pitch) --
/// whose range is exactly the arcsine's -- without a wrapping reinterpretation.
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
/// coordinate over the larger and unfolded by octant. See [`rad_to_bits`] for
/// the sign convention.
///
/// 32-bit clean, for the reasons [`sin_fast_q30`] sets out, which is why the
/// coordinates are `i32` where [`atan2_bits`] takes `i64`. The one division is
/// deliberate: a `u32` divide is a single instruction on a CPU and slow but
/// serviceable on a GPU, and avoiding it would cost more code than it saves.
pub(crate) const fn atan2_fast_bits(y: i32, x: i32, bits: u32) -> i32 {
    /// Fractional bits of the turn accumulator.
    ///
    /// Q30 keeps a full turn, a half turn and a quarter turn all representable
    /// as `i32`, which the unfolding below needs.
    const T: u32 = 30;
    /// One turn for the approximation.
    const TURN: i32 = 1 << T;
    /// Fractional bits of the ratio of the two coordinates.
    const R: u32 = 15;
    /// Bits the divisor is normalized to, so `numerator << R` fits a `u32`.
    const DIVISOR_BITS: u32 = 16;
    /// `0.273 / (2 * pi)`, the polynomial's correction weight, expressed in
    /// turns, in Q17. Derived from [`TWO_PI`] by integer division rather than
    /// written down, so this file holds no floating-point constant.
    ///
    /// The 128-bit arithmetic is `const`: it runs at compile time and leaves a
    /// literal behind, so it costs nothing at runtime and constrains no port.
    const CORRECTION: i32 = (((273_i128 << 17) << Q) / (1000 * TWO_PI as i128)) as i32;

    if x == 0 && y == 0 {
        return 0;
    }

    let ax = x.unsigned_abs();
    let ay = y.unsigned_abs();
    let steep = ay > ax;
    let (numerator, denominator) = if steep { (ax, ay) } else { (ay, ax) };

    // Normalizing the divisor to sixteen bits keeps `numerator << R` inside a
    // u32, so the ratio costs one 32-bit division. The bits dropped from the
    // divisor cost 2^-16 of relative error, two orders of magnitude under this
    // approximation's own.
    let excess = (32 - denominator.leading_zeros()).saturating_sub(DIVISOR_BITS);
    let ratio = (((numerator >> excess) << R) / (denominator >> excess)) as i32;

    // `atan(r) = r/8 + C * r * (1 - r)` in turns. The wedge `r * (1 - r)` peaks
    // at a quarter, so dropping ten of its bits leaves room for the weight and
    // costs nothing measurable.
    let wedge = ratio * ((1 << R) - ratio);
    let mut turns = (ratio << (T - R - 3)) + ((CORRECTION * (wedge >> 10)) >> 7);

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

    if bits >= T {
        // A half turn is `TURN / 2`, which at 32-bit output shifts to `2^31`:
        // representable in `u32`, and correct once reinterpreted, since a phase
        // is what it is modulo a full turn.
        ((turns as u32) << (bits - T)) as i32
    } else {
        let shift = T - bits;
        let half = 1 << (shift - 1);
        if turns >= 0 {
            (turns + half) >> shift
        } else {
            -((-turns + half) >> shift)
        }
    }
}
