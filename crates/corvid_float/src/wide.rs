//! The `f64` half, which is where a value is computed before it is bound.
//!
//! A separate module rather than a suffix on every name, so that a caller
//! working a word wider says so once — `use corvid_float::wide` — and then
//! spells `wide::sqrt` the way it would spell `sqrt`.

use const_soft_float::soft_f64::SoftF64;

/// The `f64` constants, as [`core`] spells them.
pub use core::f64::consts;

/// The square root. Negative inputs give `NaN`.
///
/// ```
/// use corvid_float::wide;
///
/// const ROOT: f64 = wide::sqrt(9.0);
/// assert_eq!(ROOT, 3.0);
/// ```
#[must_use]
#[inline]
pub const fn sqrt(x: f64) -> f64 {
    SoftF64(x).sqrt().to_f64()
}

/// The sine, in radians. Not the workspace's trigonometry — see
/// [`corvid_float::sin`](crate::sin).
#[must_use]
#[inline]
pub const fn sin(x: f64) -> f64 {
    SoftF64(x).sin().to_f64()
}

/// The cosine, in radians. Not the workspace's trigonometry — see
/// [`corvid_float::sin`](crate::sin).
#[must_use]
#[inline]
pub const fn cos(x: f64) -> f64 {
    SoftF64(x).cos().to_f64()
}

/// The tangent, in radians. Large and finite at the pole rather than infinite,
/// for the reason [`corvid_float::tan`](crate::tan) sets out.
#[must_use]
#[inline]
pub const fn tan(x: f64) -> f64 {
    let value = SoftF64(x);
    value.sin().div(value.cos()).to_f64()
}

/// The reciprocal. Zero gives an infinity rather than a panic.
#[must_use]
#[inline]
pub const fn recip(x: f64) -> f64 {
    SoftF64(1.0).div(SoftF64(x)).to_f64()
}

/// The length of the hypotenuse: `sqrt(x² + y²)`, composed naively.
///
/// Naively in [`corvid_float::hypot`](crate::hypot)'s sense, and with the same
/// two failures at the ends of the range — but a word wider those ends are at
/// about `1.3e154` and `1.5e-154` rather than at `1.8e19` and `1.1e-19`, which
/// is far enough out that no caller of this module will find them.
#[must_use]
#[inline]
pub const fn hypot(x: f64, y: f64) -> f64 {
    let (x, y) = (SoftF64(x), SoftF64(y));
    x.mul(x).add(y.mul(y)).sqrt().to_f64()
}

/// `x` raised to an integer power.
#[must_use]
#[inline]
pub const fn powi(x: f64, n: i32) -> f64 {
    SoftF64(x).powi(n).to_f64()
}

/// The largest integer no greater than `x`.
#[must_use]
#[inline]
pub const fn floor(x: f64) -> f64 {
    SoftF64(x).floor().to_f64()
}

/// The smallest integer no less than `x`.
#[must_use]
#[inline]
pub const fn ceil(x: f64) -> f64 {
    -SoftF64(-x).floor().to_f64()
}

/// `x` rounded to the nearest integer, halves away from zero.
///
/// A true round, which is worth saying because the obvious `const` spelling is
/// not one. `corvid_fixed`'s `from_f64` conversions want the same rule and
/// reach it without this crate — they add a half and let the cast to an integer
/// truncate, which is `const` because a cast is. The two agree everywhere
/// except just below a half, where adding the half rounds up into one and the
/// truncation then keeps it: `0.499_999_999_999_999_94` is zero here and one
/// there. Nothing on this branch makes `corvid_fixed` depend on `corvid_float`,
/// so that difference is currently a fact about two implementations rather than
/// a bug in either.
///
/// ```
/// use corvid_float::wide;
///
/// const UP: f64 = wide::round(0.5);
/// const DOWN: f64 = wide::round(-0.5);
/// assert_eq!(UP, 1.0);
/// assert_eq!(DOWN, -1.0);
/// ```
#[must_use]
#[inline]
pub const fn round(x: f64) -> f64 {
    SoftF64(x).round().to_f64()
}

/// `x` with its fractional part discarded.
#[must_use]
#[inline]
pub const fn trunc(x: f64) -> f64 {
    SoftF64(x).trunc().to_f64()
}

/// `x` with `sign`'s sign.
#[must_use]
#[inline]
pub const fn copysign(x: f64, sign: f64) -> f64 {
    SoftF64(x).copysign(SoftF64(sign)).to_f64()
}

/// The magnitude of `x`.
#[must_use]
#[inline]
pub const fn abs(x: f64) -> f64 {
    copysign(x, 1.0)
}

/// `x` held between `low` and `high`, without a panic when they cross. The
/// upper bound is tested first and `NaN` gives `low` — see
/// [`corvid_float::clamp`](crate::clamp).
#[must_use]
#[inline]
pub const fn clamp(x: f64, low: f64, high: f64) -> f64 {
    if x > high {
        high
    } else if x >= low {
        x
    } else {
        low
    }
}
