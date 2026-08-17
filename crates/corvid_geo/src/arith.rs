//! The integer helpers the geodetic conversions are built out of.
//!
//! Everything here is `const`, total and integer-only, and every quantity is a
//! bit pattern at a stated binary scale rather than a number with units. Three
//! scales appear: **Q16** is a metre count matching [`I48F16`](corvid_fixed::I48F16),
//! **Q48** is a dimensionless ratio near one, and a
//! [`Signed32`] bit pattern is over [`UNIT`] rather than over a power of two.
//!
//! Q48 is not a taste. The eccentricity squared of WGS84 is `0.0066943799901`,
//! and the prime vertical radius it feeds is about `6.4e6` metres, so an error
//! of one part in `2^48` in that constant moves a position by `2.3e-8` metres.
//! At Q30 -- the finest fixed-point type this workspace has -- the same
//! constant would move it by 1.5 millimetres, which is half of what
//! [`GlobalPoint`](corvid_vector::GlobalPoint) can even represent.

use corvid_fixed::Signed32;

/// The divisor a [`Signed32`] bit pattern is over.
///
/// `SNORM` divides by `2^31 - 1` rather than by `2^31`, so a sine cannot be
/// rescaled with a shift. Treating it as a power of two would be a relative
/// error of `4.7e-10`, which at the earth's radius is three millimetres -- the
/// whole resolution of the type the answer lands in.
pub(crate) const UNIT: i128 = i32::MAX as i128;

/// Divides, rounding half away from zero.
///
/// A zero or negative `denominator` answers zero rather than dividing. No
/// caller in this crate can produce one -- every denominator is either a
/// positive constant or a square root of a value bounded below by `1 - e^2` --
/// and answering keeps the function total under the workspace's `panic` lint.
pub(crate) const fn round_div(numerator: i128, denominator: i128) -> i128 {
    if denominator <= 0 {
        return 0;
    }
    let half = denominator / 2;
    if numerator < 0 {
        (numerator - half) / denominator
    } else {
        (numerator + half) / denominator
    }
}

/// A Q16 metre count multiplied by a sine or a cosine, rounded once.
pub(crate) const fn scale_by(value: i64, signed: Signed32) -> i64 {
    round_div(value as i128 * signed.to_bits() as i128, UNIT) as i64
}

/// The square of a sine or a cosine, in Q48.
///
/// `s^2 << 48` reaches `1.3e33`, which is why the intermediate is an `i128`
/// and why this is not written as two `scale_by` calls.
pub(crate) const fn square_q48(signed: Signed32) -> i128 {
    let s = signed.to_bits() as i128;
    round_div((s * s) << 48, UNIT * UNIT)
}

/// The cube of a sine or a cosine, in Q48.
pub(crate) const fn cube_q48(signed: Signed32) -> i128 {
    round_div(square_q48(signed) * signed.to_bits() as i128, UNIT)
}

/// A Q16 metre count multiplied by a Q48 ratio, rounded once.
pub(crate) const fn scale_q48(value: i64, ratio: i128) -> i64 {
    round_div(value as i128 * ratio, 1 << 48) as i64
}

/// Narrows a Q16 metre count to the Q8 one [`I24F8`](corvid_fixed::I24F8)
/// holds, rounding half away from zero.
///
/// A division rather than a shift, and the difference is the whole point: an
/// arithmetic shift right *floors*, so a negative height would round down
/// rather than toward the nearer step and a cellar would sit one bit deeper
/// than a cornice of the same size sits high.
pub(crate) const fn q16_to_q8(value: i64) -> i64 {
    round_div(value as i128, 256) as i64
}

/// Shifts a ratio down until both halves fit an `i64`, keeping 63 significant
/// bits of the larger.
///
/// [`Angle32::atan2`](corvid_fixed::Angle32::atan2) takes its arguments as a
/// pair of `i64` and answers from their ratio alone, so the scale is free to
/// be chosen -- but a Q16 metre count times a Q16 radius reaches `4e23`, which
/// is not one. Shifting both by the same amount leaves the ratio intact and
/// the arctangent's own 32-bit output far coarser than what the shift costs.
pub(crate) const fn fit_ratio(y: i128, x: i128) -> (i64, i64) {
    let widest = if y.unsigned_abs() > x.unsigned_abs() {
        y.unsigned_abs()
    } else {
        x.unsigned_abs()
    };
    let significant = 128 - widest.leading_zeros();
    if significant <= 63 {
        return (y as i64, x as i64);
    }
    let down = significant - 63;
    ((y >> down) as i64, (x >> down) as i64)
}
