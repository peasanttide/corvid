//! The `f32` half, which is the half a device sees.
//!
//! Every function is `const`. The ones that are arithmetic go through
//! [`SoftF32`], because an intrinsic cannot be called in a const context; the
//! two clamps are written out here, because a comparison is const already and
//! routing one through the soft float would only be slower. The sign
//! operations go through [`SoftF32`] as well even though a sign is a bit and
//! not an intrinsic -- upstream's is the same masking this crate would write,
//! and one implementation of it is easier to keep right than two.

use const_soft_float::soft_f32::SoftF32;

// The intrinsics this module stands in for are named below as reference links
// with a literal URL rather than as intra-doc links. That is not a stylistic
// preference: `f32::sqrt` and `f32::hypot` are inherent methods `std` adds to
// the primitive, not `core` ones, and this crate is `#![no_std]` -- so rustdoc
// has no `std` in the graph to resolve them against and `cargo doc -D warnings`
// rejects the build. `f32::clamp` a few functions down *is* a `core` method and
// links the ordinary way, which is the whole distinction. Anything that reads
// like a link here has to be checked against `core`, not against memory.

/// The square root. Negative inputs give `NaN`, as [`f32::sqrt`] does.
///
/// ```
/// const ROOT: f32 = corvid_float::sqrt(9.0);
/// assert_eq!(ROOT, 3.0);
/// ```
///
/// [`f32::sqrt`]: https://doc.rust-lang.org/std/primitive.f32.html#method.sqrt
#[must_use]
#[inline]
pub const fn sqrt(x: f32) -> f32 {
    SoftF32(x).sqrt().to_f32()
}

/// The sine, in radians.
///
/// **This is not the workspace's trigonometry.** A simulation's angles are
/// [`corvid_fixed`]'s, and its sine is the integer CORDIC there, because two
/// machines have to agree on the answer bit for bit. This one is for the
/// boundary -- a projection's focal length, a gain curve -- where nothing is
/// compared against another machine.
///
/// [`corvid_fixed`]: https://docs.rs/corvid_fixed
#[must_use]
#[inline]
pub const fn sin(x: f32) -> f32 {
    SoftF32(x).sin().to_f32()
}

/// The cosine, in radians. [`sin`]'s note about which trigonometry this is
/// applies here too.
#[must_use]
#[inline]
pub const fn cos(x: f32) -> f32 {
    SoftF32(x).cos().to_f32()
}

/// The tangent, in radians.
///
/// Composed rather than supplied: `sin` over `cos`. It is a division, so a
/// cosine of zero would give an infinity rather than a panic.
///
/// The cosine does not reach zero, though, and that is the trap worth knowing
/// about. No `f32` is pi/2, so at the nearest one the cosine is merely small and
/// the tangent is a large *finite* number of whichever sign the rounding landed
/// on: `tan(consts::FRAC_PI_2)` is about `-2.3e7`, negative because the nearest
/// `f32` sits just past pi/2. A caller building a frustum from a field of view
/// of half a turn gets a finite, sign-flipped focal length, not the infinity a
/// finiteness test would catch -- so bound the angle before taking its tangent
/// rather than screening the result. [`clamp`] is here for that.
///
/// ```
/// const FLAT: f32 = corvid_float::tan(0.0);
/// assert_eq!(FLAT, 0.0);
/// ```
#[must_use]
#[inline]
pub const fn tan(x: f32) -> f32 {
    let value = SoftF32(x);
    value.sin().div(value.cos()).to_f32()
}

/// The reciprocal. Zero gives an infinity rather than a panic.
#[must_use]
#[inline]
pub const fn recip(x: f32) -> f32 {
    SoftF32(1.0).div(SoftF32(x)).to_f32()
}

/// The length of the hypotenuse: `sqrt(x^2 + y^2)`.
///
/// Composed rather than supplied, and composed naively -- the squares are formed
/// before the root, where [`f32::hypot`] scales its arguments first so that they
/// never have to be. Skipping that costs both ends of the range, not just the
/// top. Above about `1.8e19` a square overflows and this returns an infinity
/// where the intrinsic returns the answer; below about `1.1e-19` a square is
/// subnormal and starts shedding bits, and by `f32::MIN_POSITIVE` it has
/// collapsed entirely -- `hypot(f32::MIN_POSITIVE, f32::MIN_POSITIVE)` is `0.0`
/// here and `1.66e-38` there, which is the quieter of the two failures and so
/// the one worth writing down.
///
/// Between those bounds the only error is the extra roundings the composition
/// adds, which the tests hold to a millionth of the answer -- and a camera
/// working in metres never leaves them.
///
/// [`f32::hypot`]: https://doc.rust-lang.org/std/primitive.f32.html#method.hypot
#[must_use]
#[inline]
pub const fn hypot(x: f32, y: f32) -> f32 {
    let (x, y) = (SoftF32(x), SoftF32(y));
    x.mul(x).add(y.mul(y)).sqrt().to_f32()
}

/// `x` raised to an integer power.
#[must_use]
#[inline]
pub const fn powi(x: f32, n: i32) -> f32 {
    SoftF32(x).powi(n).to_f32()
}

/// The largest integer no greater than `x`.
#[must_use]
#[inline]
pub const fn floor(x: f32) -> f32 {
    SoftF32(x).floor().to_f32()
}

/// The smallest integer no less than `x`.
///
/// Composed rather than supplied: the floor of the negation, negated. That
/// identity is exact in binary floating point -- a negation is a sign bit -- so
/// this is the ceiling rather than an approximation of it.
///
/// ```
/// const UP: f32 = corvid_float::ceil(1.2);
/// assert_eq!(UP, 2.0);
/// ```
#[must_use]
#[inline]
pub const fn ceil(x: f32) -> f32 {
    -SoftF32(-x).floor().to_f32()
}

/// `x` rounded to the nearest integer, halves away from zero.
#[must_use]
#[inline]
pub const fn round(x: f32) -> f32 {
    SoftF32(x).round().to_f32()
}

/// `x` with its fractional part discarded.
#[must_use]
#[inline]
pub const fn trunc(x: f32) -> f32 {
    SoftF32(x).trunc().to_f32()
}

/// `x` with `sign`'s sign.
#[must_use]
#[inline]
pub const fn copysign(x: f32, sign: f32) -> f32 {
    SoftF32(x).copysign(SoftF32(sign)).to_f32()
}

/// The magnitude of `x`.
///
/// A sign bit rather than a soft-float call, and written as a `copysign` onto a
/// positive one so that it is a bit operation rather than a comparison -- which
/// is what makes it right for `-0.0` as well as for `NaN`.
#[must_use]
#[inline]
pub const fn abs(x: f32) -> f32 {
    copysign(x, 1.0)
}

/// `x` held between `low` and `high`.
///
/// Unlike [`f32::clamp`] this does not panic when `low` exceeds `high`,
/// because the workspace forbids a panic in a library and a caller that has
/// crossed its own bounds is better served by a value than by a dead process.
/// The upper bound is tested first, so crossed bounds give `high` for any `x`
/// above it and `low` for anything else.
///
/// `NaN` gives `low`. Every comparison against it is false, so it falls
/// through both arms -- which is the useful answer as well as the one the
/// branch order produces: a gain that has gone to `NaN` should come back as
/// silence rather than as full volume.
///
/// ```
/// const HELD: f32 = corvid_float::clamp(5.0, 0.0, 1.0);
/// assert_eq!(HELD, 1.0);
/// ```
#[must_use]
#[inline]
pub const fn clamp(x: f32, low: f32, high: f32) -> f32 {
    if x > high {
        high
    } else if x >= low {
        x
    } else {
        low
    }
}

/// `x` held between `low` and `high`, with anything non-finite going to `low`.
///
/// [`clamp`]'s sibling, and the first difference is what happens to an
/// infinity: there it is above `high` and comes back as `high`, here it comes
/// back as `low` along with `NaN`.
///
/// The second difference only shows when the bounds cross, and it is the order
/// they are tested in. [`clamp`] tests `high` first, so crossed bounds give
/// `high` to anything above it and `low` to everything else; this one tests
/// `low` first, so crossed bounds give `low` to anything below it and `high` to
/// everything else. Neither order is the correct answer to a question that is
/// already wrong, but the two are not the same wrong answer and a caller moving
/// between them should know that.
///
/// That is the right reading wherever a large value is the dangerous one. A
/// frequency, a decay and a gain that arrived as infinities are a caller or a
/// device that has malfunctioned, and the quietest interpretation of a
/// malfunction is the one that does not scream -- which is why the audio mixer
/// wants this one and a matrix does not.
///
/// ```
/// const LOUD: f32 = corvid_float::clamp_finite(f32::INFINITY, 0.0, 1.0);
/// assert_eq!(LOUD, 0.0);
/// ```
#[must_use]
#[inline]
pub const fn clamp_finite(x: f32, low: f32, high: f32) -> f32 {
    if !x.is_finite() || x < low {
        low
    } else if x > high {
        high
    } else {
        x
    }
}

/// An [`f64`] as the [`f32`] a device takes.
///
/// The one narrowing in this crate, and it is named for what it does rather
/// than spelled `as` at each site -- a texture coordinate, a matrix entry and a
/// gain are all computed a word wider than they are bound.
///
/// A magnitude past `f32`'s range narrows to an infinity rather than to
/// [`f32::MAX`]. That is Rust's cast and not a decision made here, but it is
/// the reason [`clamp_finite`] exists: a gain that overflowed on the way down
/// from `f64` arrives as an infinity, and something has to turn it back into a
/// number before a device sees it.
///
/// ```
/// const THIRD: f32 = corvid_float::demote(1.0 / 3.0);
/// assert_eq!(THIRD, 1.0_f32 / 3.0);
/// ```
#[must_use]
#[inline]
#[expect(
    clippy::cast_possible_truncation,
    reason = "the truncation is the function: a value computed in f64 and bound to a device as f32 loses the mantissa it never had a use for, and naming the step is what this exists to do"
)]
pub const fn demote(x: f64) -> f32 {
    x as f32
}
