//! The rotation operation family: axis-angle, Euler, `look_to`, arcs and steps.
//!
//! Everything is written once against [`Versor`] and forwarded from [`Basis`],
//! except where the matrix form is strictly better -- [`Basis::look_to`] and
//! [`Basis::from_yaw_pitch_roll`] build the matrix directly, because their
//! answer *is* a set of axes.
//!
//! # Conventions this module nails down
//!
//! Yaw rotates about **+Z**, pitch about **+X**, roll about **+Y**, and Euler
//! composition is **ZXY intrinsic**: `R = Rz(yaw) * Rx(pitch) * Ry(roll)`. Yaw
//! and roll take [`Angle32`] because they wrap; pitch takes [`Pitch32`] because
//! it clamps, which is exactly right for a head pose -- looking too far up
//! leaves you looking up rather than upside down.
//!
//! `right = forward x up`, consistent with `X x Y = Z`.

use corvid_fixed::{Angle32, I2F30, Signed32};
use corvid_vector::Direction;

use crate::basis::{round_shift_i64, signed_from_q30};
use crate::normalize::shift_down;

/// `1.0` at the Q30 scale.
const ONE: i64 = 1 << 30;

/// How close `sin(pitch)` must come to `+/-1` before yaw and roll are treated as
/// degenerate, in Q30 last bits.
///
/// Derived rather than picked. Outside the branch `to_yaw_pitch_roll` reads
/// yaw off `atan2(-m01, m11)`, whose two arguments are both `cos(pitch)` times
/// something bounded by one; the quantization floor is a Q30 last bit, so the
/// bearing carries about `log2(cos(pitch) * 2^30)` bits. With
/// `|m21| = 1 - k/2^30`, `cos(pitch) ~= sqrt(2k)*2^-15`, so `k = 1 << 7` leaves
/// `cos(pitch) >= 4.9e-4` -- 19 bits, an angular floor near `1e-4 deg`, comfortably
/// under the `0.005 deg` the codec itself carries.
///
/// The previous `1 << 12` fired from **89.84 deg**, where `cos(pitch)` is still
/// `2.8e-3` and roll is fully determined; discarding it and attributing the
/// whole turn to yaw cost `0.30 deg` of round-trip error -- 60x the codec floor --
/// in a band head tracking passes through routinely.
const POLE_MARGIN: i64 = 1 << 7;

/// Converts a [`Signed32`] into a Q30 bit pattern, rounded once.
///
/// Reads the *canonical* bit pattern. `Signed32` spends `i32::MIN` and
/// `-(2^31 - 1)` on the same `-1.0` and folds the denormal on the way into its
/// own arithmetic; this does the same, so two components that compare and hash
/// equal cannot produce different rotations.
#[inline]
const fn q30_from_signed(value: Signed32) -> i64 {
    let scaled = (value.canonicalize().to_bits() as i64) << 30;
    let denominator = Signed32::MAX.to_bits() as i64;
    if scaled >= 0 {
        (2 * scaled + denominator) / (2 * denominator)
    } else {
        -((-2 * scaled + denominator) / (2 * denominator))
    }
}
mod basis;
mod versor;

/// Twice the `acos` of a non-negative Q30 cosine: the angle between two
/// rotations, in `0 ..= half a turn`.
#[inline]
const fn angle_from_cosine(cosine: i64) -> Angle32 {
    let half = Angle32::acos(signed_from_q30(cosine as i32));
    Angle32::from_bits(half.to_bits().wrapping_mul(2))
}

/// Some unit vector perpendicular to `v`, at Q30.
///
/// Crosses with whichever cardinal axis `v` leans on least, which is never
/// degenerate.
#[inline]
const fn perpendicular_to(v: [i64; 3]) -> [i64; 3] {
    if v[0].abs() <= v[1].abs() && v[0].abs() <= v[2].abs() {
        [0, -v[2], v[1]]
    } else if v[1].abs() <= v[2].abs() {
        [-v[2], 0, v[0]]
    } else {
        [-v[1], v[0], 0]
    }
}
/// The product of three Q30 values, brought back to Q60.
#[inline]
const fn mul3(a: i64, b: i64, c: i64) -> i64 {
    round_shift_i64(a * b, 30) * c
}

/// A `Signed32` axis component as an `I2F30` matrix entry.
#[inline]
const fn axis_entry(value: Signed32) -> I2F30 {
    I2F30::from_bits(q30_from_signed(value) as i32)
}

/// `a x b`, normalized, without the round trip through a unit-scaled
/// [`Direction`].
///
/// [`Direction::cross`] divides its `i64` cross terms back onto `Signed32`'s
/// `+/-1` before returning, which keeps only the bits *above* the cross product's
/// own magnitude. For two nearly parallel directions that magnitude is tiny --
/// it goes as the sine of the angle between them -- so almost nothing survives
/// the division, and the `normalize` that follows amplifies what is left of the
/// rounding rather than a direction. At `0.006 deg` of separation that made
/// [`Basis::look_to`] hand back a frame skewed by a third of a degree, and a
/// tenth of that separation cost ten.
///
/// Rescaling the terms by a shift instead of dividing them keeps every bit the
/// `i64` products carried; [`Direction::normalize`] cares only about ratios, so
/// the shift costs nothing at all.
///
/// `None` when the two are parallel -- including when either is zero -- which is
/// the only case with no answer.
#[inline]
const fn cross_normalized(a: Direction, b: Direction) -> Option<Direction> {
    // Canonical bits: `Signed32` spends `i32::MIN` and `-(2^31 - 1)` on the
    // same `-1.0`, and reading the raw pattern would make two components that
    // compare equal cross to different axes.
    let ax = a.x().canonicalize().to_bits() as i64;
    let ay = a.y().canonicalize().to_bits() as i64;
    let az = a.z().canonicalize().to_bits() as i64;
    let bx = b.x().canonicalize().to_bits() as i64;
    let by = b.y().canonicalize().to_bits() as i64;
    let bz = b.z().canonicalize().to_bits() as i64;

    // Each product is at most `(2^31 - 1)^2` and a difference of two of them
    // reaches `2 * (2^31 - 1)^2`, which is still under `i64::MAX`.
    let c = [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx];

    let mut largest = c[0].unsigned_abs();
    let mut i = 1;
    while i < 3 {
        if c[i].unsigned_abs() > largest {
            largest = c[i].unsigned_abs();
        }
        i += 1;
    }
    if largest == 0 {
        return None;
    }

    // Bring the largest term into `[2^30, 2^31)` so the whole triple survives
    // the narrowing to `i32` with every bit it had.
    let bit_length = 64 - largest.leading_zeros();
    let scaled = if bit_length > 31 {
        let down = bit_length - 31;
        [
            shift_down(c[0], down),
            shift_down(c[1], down),
            shift_down(c[2], down),
        ]
    } else {
        let up = 31 - bit_length;
        [c[0] << up, c[1] << up, c[2] << up]
    };

    Direction::new(
        Signed32::from_bits(scaled[0] as i32),
        Signed32::from_bits(scaled[1] as i32),
        Signed32::from_bits(scaled[2] as i32),
    )
    .normalize()
}
