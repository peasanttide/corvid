//! Bringing a wide intermediate back to the width a component is.
//!
//! Fixed-point arithmetic widens: a Q16 times a Q16 is a Q32 that needs an
//! `i64` to hold it, and a three-component dot product of those needs an
//! `i128`. The result is a component again, and a component is an `i32`. This
//! is the step back down.
//!
//! Two answers, because two are wanted. A camera whose position has run past
//! what an `i32` reaches should clamp — the far edge of the representable world
//! is a truer answer than a wrap, and it is what the frustum and the input
//! scaling do. A conversion that is being *checked* should say so instead,
//! which is what [`try_narrow_i64`] is for and what `GlobalPoint`'s widening
//! round trip needs.
//!
//! Wrapping is not offered. It is the one answer that is never right here: a
//! position that wraps puts an object on the opposite side of the world, and
//! silently.

/// A wide intermediate as an [`i32`], clamping rather than wrapping.
///
/// ```
/// use corvid_bits::narrow_i64;
///
/// assert_eq!(narrow_i64(7), 7);
/// assert_eq!(narrow_i64(i64::from(i32::MAX) + 1), i32::MAX);
/// assert_eq!(narrow_i64(i64::MIN), i32::MIN);
/// ```
#[must_use]
#[inline]
#[expect(
    clippy::cast_possible_truncation,
    reason = "the two arms above the cast are what establish the range, so the cast is the narrowing they guard rather than an unchecked one"
)]
pub const fn narrow_i64(value: i64) -> i32 {
    if value > i32::MAX as i64 {
        i32::MAX
    } else if value < i32::MIN as i64 {
        i32::MIN
    } else {
        value as i32
    }
}

/// A wide intermediate as an [`i32`], clamping rather than wrapping.
///
/// ```
/// use corvid_bits::narrow_i128;
///
/// assert_eq!(narrow_i128(-7), -7);
/// assert_eq!(narrow_i128(i128::from(i32::MIN) - 1), i32::MIN);
/// assert_eq!(narrow_i128(i128::MAX), i32::MAX);
/// ```
#[must_use]
#[inline]
#[expect(
    clippy::cast_possible_truncation,
    reason = "the two arms above the cast are what establish the range, so the cast is the narrowing they guard rather than an unchecked one"
)]
pub const fn narrow_i128(value: i128) -> i32 {
    if value > i32::MAX as i128 {
        i32::MAX
    } else if value < i32::MIN as i128 {
        i32::MIN
    } else {
        value as i32
    }
}

/// A wide intermediate as an [`i32`], or [`None`] if it does not fit.
///
/// ```
/// use corvid_bits::try_narrow_i64;
///
/// assert_eq!(try_narrow_i64(7), Some(7));
/// assert_eq!(try_narrow_i64(i64::from(i32::MAX)), Some(i32::MAX));
/// assert_eq!(try_narrow_i64(i64::from(i32::MAX) + 1), None);
/// ```
#[must_use]
#[inline]
#[expect(
    clippy::cast_possible_truncation,
    reason = "the guard above the cast rejects every value that would not survive it, so the cast is exact wherever it is reached"
)]
pub const fn try_narrow_i64(value: i64) -> Option<i32> {
    if value > i32::MAX as i64 || value < i32::MIN as i64 {
        None
    } else {
        Some(value as i32)
    }
}

/// A wide intermediate as an [`i32`], or [`None`] if it does not fit.
///
/// ```
/// use corvid_bits::try_narrow_i128;
///
/// assert_eq!(try_narrow_i128(i128::from(i32::MIN)), Some(i32::MIN));
/// assert_eq!(try_narrow_i128(i128::from(i32::MIN) - 1), None);
/// ```
#[must_use]
#[inline]
#[expect(
    clippy::cast_possible_truncation,
    reason = "the guard above the cast rejects every value that would not survive it, so the cast is exact wherever it is reached"
)]
pub const fn try_narrow_i128(value: i128) -> Option<i32> {
    if value > i32::MAX as i128 || value < i32::MIN as i128 {
        None
    } else {
        Some(value as i32)
    }
}
