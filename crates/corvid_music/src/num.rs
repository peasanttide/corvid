//! The conversions between a float and an index, gathered in one place.
//!
//! Music is counted and sound is measured, so this crate crosses between the
//! two constantly: a beat is a float and a note is a `u8`, a second is a float
//! and a frame is a `u64`. Every one of those crossings loses something, and
//! the loss is only safe because of where the value came from. Writing them
//! once, here, is what lets the reason be stated once rather than at every
//! `as`.

/// The highest MIDI key number.
#[cfg(feature = "compose")]
pub(crate) const MAX_KEY: u8 = 127;

/// Rounds to the nearest MIDI key, clamping into `0 ..= 127`.
///
/// The clamp is what makes the cast total: a transposition that walks off the
/// keyboard lands on its end rather than wrapping to the other one, which is
/// audible as a line flattening out instead of as a shriek.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the clamp above the cast leaves only 0 ..= 127, which every u8 \
              holds exactly"
)]
#[cfg(feature = "compose")]
pub(crate) fn key(value: f32) -> u8 {
    let rounded = libm::roundf(value);
    if rounded <= 0.0 {
        0
    } else if rounded >= f32::from(MAX_KEY) {
        MAX_KEY
    } else {
        rounded as u8
    }
}

/// Rounds a non-negative count to the nearest `usize`.
///
/// A negative or non-finite input answers zero, because every caller here is
/// asking "how many", and the honest answer to a nonsense length is none.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the guard above the cast leaves a finite non-negative value below \
              the ceiling, which is what the cast needs"
)]
#[cfg(feature = "compose")]
pub(crate) fn count(value: f32) -> usize {
    let rounded = libm::roundf(value);
    if rounded <= 0.0 || !rounded.is_finite() {
        0
    } else if rounded >= CEILING {
        CEILING_COUNT
    } else {
        rounded as usize
    }
}

/// The largest count this module will produce, as a float and as an index.
///
/// Two million is past anything a bar of music or a block of audio asks for and
/// far below `f32`'s integer limit, so the guard never rejects a real request
/// and never lets a runaway one allocate.
const CEILING: f32 = 2_097_152.0;
/// [`CEILING`] as the index it clamps to.
const CEILING_COUNT: usize = 2_097_152;

/// Widens a count into the float arithmetic that measures it.
#[expect(
    clippy::cast_precision_loss,
    reason = "a count above 2^24 would round, and every count here is a note in \
              a bar or a frame in a block"
)]
#[cfg(feature = "compose")]
pub(crate) fn of(value: usize) -> f32 {
    value as f32
}

/// Widens a frame counter into the float arithmetic that measures it.
///
/// Above 2^24 frames -- about six minutes at 48 kHz -- this rounds, which is
/// why nothing schedules against an absolute frame in seconds. It is used for
/// offsets within a block.
#[expect(
    clippy::cast_precision_loss,
    reason = "stated in the doc comment: the caller passes a within-block offset"
)]
pub(crate) fn of_u32(value: u32) -> f32 {
    value as f32
}

/// A whole read position, as an index into a sample's frames.
///
/// The position is already floored by the caller and is clamped here into
/// `0 ..= CEILING_COUNT`, so a runaway increment reads past the end -- which
/// the caller treats as silence -- rather than wrapping to the beginning.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the clamp above the cast leaves a whole non-negative value below \
              the ceiling"
)]
#[cfg(feature = "synth")]
pub(crate) fn frame(position: f64) -> usize {
    if position <= 0.0 || !position.is_finite() {
        0
    } else if position >= f64::from(CEILING) {
        CEILING_COUNT
    } else {
        position as usize
    }
}

/// Narrows a fractional read position into the float the interpolation runs in.
///
/// The value is a fraction of one frame, so it is in `0.0 ..= 1.0` and every bit
/// an `f32` can hold of it is a bit that matters.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is a fraction in 0.0 ..= 1.0, where f32 loses only \
              precision the interpolation never had"
)]
#[cfg(feature = "synth")]
pub(crate) fn narrow(fraction: f64) -> f32 {
    fraction as f32
}

/// Widens a generator amount into the units it is measured in.
///
/// A `SoundFont` generator is sixteen bits in the file and is widened to `i32`
/// only so that a preset's additive layer cannot overflow, so every value that
/// reaches here is well inside the range an `f32` holds exactly.
#[expect(
    clippy::cast_precision_loss,
    reason = "stated in the doc comment: the value came from an i16 and a sum \
              of i16s"
)]
#[cfg(feature = "synth")]
pub(crate) fn of_i32(value: i32) -> f32 {
    value as f32
}
