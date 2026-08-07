//! How long a tick lasts, and the rate that follows from it.

use core::num::{NonZeroU32, NonZeroU64};
use core::time::Duration;

/// Nanoseconds in a second, which is what a rate is converted through.
const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// Builds a [`NonZeroU64`] in a `const` context without an `unwrap`.
///
/// `NonZeroU64::new` returns an `Option`, and unwrapping one is denied by the
/// workspace lints for the same reason it is denied everywhere else. A span of
/// no time at all is a division by zero in [`Step`](crate::Step), so the
/// fallback is the shortest span there is.
const fn nonzero(nanos: u64) -> NonZeroU64 {
    match NonZeroU64::new(nanos) {
        Some(nanos) => nanos,
        None => NonZeroU64::MIN,
    }
}

/// How long one tick of the simulation lasts.
///
/// The span is part of a session's opening rather than a runtime setting: two
/// peers at different spans are two different simulations, and a replay
/// recorded at one span does not mean anything at another. That is why this is
/// a hashable value with a wire format and not a number in a config struct.
///
/// # Why the span and not the rate
///
/// This used to be a `TickRate` holding a `NonZeroU32` of hertz, deriving the
/// span by dividing into a second. That division truncates — fifteen hertz is
/// 66 666 666 nanoseconds and not a fifteenth of a second — so the stored
/// value was the *approximate* one and the exact one was derived from it.
///
/// It is the wrong way round. [`Step`](crate::Step) accumulates against the
/// span and nothing accumulates against the rate, so the span is the number the
/// simulation is actually defined by. Storing it means a span is whatever it
/// says it is, [`hz`](Self::hz) becomes the lossy view rather than the stored
/// truth, and a game wanting a period no whole rate names — a 72 Hz headset's
/// 13 888 888 ns, say — can hold one exactly.
///
/// ```
/// use core::num::NonZeroU32;
/// use core::time::Duration;
/// use corvid_time::TickSpan;
///
/// // A zero rate has no span, so the conversion refuses one.
/// const FAST: TickSpan = match NonZeroU32::new(60) {
///     Some(hz) => TickSpan::from_hz(hz),
///     None => TickSpan::CRADLE,
/// };
///
/// assert_eq!(TickSpan::CRADLE.hz(), 15);
/// assert_eq!(TickSpan::CRADLE.period(), Duration::from_nanos(66_666_666));
/// assert_eq!(FAST.period(), Duration::from_nanos(16_666_666));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(::serde::Serialize, ::serde::Deserialize),
    serde(transparent)
)]
pub struct TickSpan(NonZeroU64);

impl TickSpan {
    /// Sixty-six milliseconds and change: fifteen ticks a second, the span
    /// Corvid's own example runs at and the default everything else inherits.
    ///
    /// Fifteen hertz is low on purpose. A tick has sixty-six milliseconds to
    /// simulate in, which is room for a rollback of half a dozen ticks over
    /// fifty thousand entities inside one frame's budget, and the display is
    /// not waiting on any of it — the camera and the cursor run at the refresh
    /// rate and never ask the simulation for permission. Shortening the span
    /// buys nothing the player can see and spends the headroom rollback needs.
    ///
    /// Exactly the number `from_hz(15)` produced before this held a span, so a
    /// session at the default computes what it always did.
    pub const CRADLE: Self = Self(nonzero(NANOS_PER_SECOND / 15));

    /// The span of exactly this many nanoseconds.
    #[must_use]
    #[inline]
    pub const fn from_nanos(nanos: NonZeroU64) -> Self {
        Self(nanos)
    }

    /// The span for `hz` ticks per second, truncated to a whole nanosecond.
    ///
    /// The truncation is where a rate stops being exact: `1_000_000_000 / hz`
    /// leaves a remainder for every rate that is not a divisor of a second, so
    /// a simulation runs fast against real time by `1_000_000_000 % hz`
    /// nanoseconds per second. Ten of them at fifteen hertz, which is one
    /// second of drift every three years against a wall clock nobody is
    /// comparing the tick counter to.
    ///
    /// Rates above a gigahertz truncate to nothing, so the span floors at one
    /// nanosecond and [`Step`](crate::Step) keeps a divisor it can divide by.
    #[must_use]
    #[inline]
    #[allow(
        clippy::cast_lossless,
        reason = "`u64::from` is not a `const fn` and this has to be one, so that a span is available where a constant is required; widening a `u32` cannot lose anything, which is the whole of what the lint is guarding"
    )]
    pub const fn from_hz(hz: NonZeroU32) -> Self {
        Self(nonzero(NANOS_PER_SECOND / hz.get() as u64))
    }

    /// How long a tick lasts, in nanoseconds.
    #[must_use]
    #[inline]
    pub const fn nanos(self) -> u64 {
        self.0.get()
    }

    /// How long a tick lasts.
    #[must_use]
    #[inline]
    pub const fn period(self) -> Duration {
        Duration::from_nanos(self.0.get())
    }

    /// How many ticks run per second, truncated.
    ///
    /// The lossy direction, and the one that is a *view* rather than the value:
    /// a span of 66 666 666 nanoseconds reports fifteen, and so does every
    /// span from 62 500 001 to 66 666 666. Two spans reporting the same rate
    /// are still two different simulations, so this is for showing a player
    /// and never for comparing two sessions.
    ///
    /// Zero for a span longer than a second, which has no whole rate to name.
    #[must_use]
    #[inline]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the numerator is 1_000_000_000 and the denominator is at least one, so the quotient is at most 1e9 — a third of `u32::MAX`, and the cast is exact for every span there is"
    )]
    pub const fn hz(self) -> u32 {
        (NANOS_PER_SECOND / self.0.get()) as u32
    }
}

impl Default for TickSpan {
    /// [`CRADLE`](Self::CRADLE), fifteen ticks a second.
    #[inline]
    fn default() -> Self {
        Self::CRADLE
    }
}

/// The span of that many nanoseconds, which is
/// [`from_nanos`](TickSpan::from_nanos).
///
/// It takes the non-zero type rather than a `u64` because a span of no time is
/// a divisor of zero in [`Step`](crate::Step), and the place to refuse one is
/// the type rather than a fallible conversion every caller then has to unwrap.
impl From<NonZeroU64> for TickSpan {
    #[inline]
    fn from(nanos: NonZeroU64) -> Self {
        Self::from_nanos(nanos)
    }
}

/// The span for that many ticks a second, which is
/// [`from_hz`](TickSpan::from_hz) and is lossy — see there.
impl From<NonZeroU32> for TickSpan {
    #[inline]
    fn from(hz: NonZeroU32) -> Self {
        Self::from_hz(hz)
    }
}

/// How long a tick lasts, still known to be non-zero.
impl From<TickSpan> for NonZeroU64 {
    #[inline]
    fn from(span: TickSpan) -> Self {
        span.0
    }
}

/// How long a tick lasts, which is [`period`](TickSpan::period).
impl From<TickSpan> for Duration {
    #[inline]
    fn from(span: TickSpan) -> Self {
        span.period()
    }
}
