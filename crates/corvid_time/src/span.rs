//! How long a tick lasts, and the rate that follows from it.

use core::num::NonZeroU32;
use core::time::Duration;

/// Nanoseconds in a second, which is what a rate is converted through.
const NANOS_PER_SECOND: u32 = 1_000_000_000;

/// Builds a [`NonZeroU32`] in a `const` context without an `unwrap`.
///
/// `NonZeroU32::new` returns an `Option`, and unwrapping one is denied by the
/// workspace lints for the same reason it is denied everywhere else. A span of
/// no time at all is a division by zero in [`Step`](crate::Step), so the
/// fallback is the shortest span there is.
const fn nonzero(nanos: u32) -> NonZeroU32 {
    match NonZeroU32::new(nanos) {
        Some(nanos) => nanos,
        None => NonZeroU32::MIN,
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
/// Holding a `NonZeroU32` of hertz and deriving the span by dividing into a
/// second is the wrong way round. That division truncates -- fifteen hertz is
/// 66 666 666 nanoseconds and not a fifteenth of a second -- so the stored value
/// would be the *approximate* one and the exact one derived from it.
///
/// [`Step`](crate::Step) accumulates against the span and nothing accumulates
/// against the rate, so the span is the number the simulation is actually
/// defined by. Storing it means a span is whatever it says it is,
/// [`hz`](Self::hz) becomes the lossy view rather than the stored truth, and a
/// game wanting a period no whole rate names -- a 72 Hz headset's 13 888 888 ns,
/// say -- can hold one exactly.
///
/// # Why a `u32` of them
///
/// Nanoseconds, so the exactness above is real, and thirty-two bits of them, so
/// the range stops at [`MAX`](Self::MAX) -- four and a bit seconds. That bound is
/// load-bearing rather than incidental: [`Step::alpha`](crate::Step::alpha)
/// multiplies the accumulator by 65 535, and the accumulator is below one span,
/// so a span that fits in a `u32` makes that product fit in a `u64` with four
/// orders of magnitude to spare. A `u64` of nanoseconds would not -- that product
/// leaves the integer past about thirty-nine hours of span -- and the
/// alternatives to bounding the type are 128-bit arithmetic on every frame or a
/// clamp that silently rewrites what a caller asked for.
///
/// Nothing is lost at the slow end that a rate could have named:
/// [`from_hz`](Self::from_hz) takes a non-zero `u32`, so the slowest span it can
/// produce is one second, and this holds four of those.
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
pub struct TickSpan(NonZeroU32);

impl TickSpan {
    /// Sixty-six milliseconds and change: fifteen ticks a second, the default
    /// everything else inherits.
    ///
    /// Fifteen hertz is low on purpose. A tick has sixty-six milliseconds to
    /// simulate in, which is where a rollback of half a dozen ticks has to fit
    /// inside one frame's budget, and the display is not waiting on any of it --
    /// the camera and the cursor run at the refresh rate and never ask the
    /// simulation for permission. Shortening the span buys nothing the player
    /// can see and spends the headroom rollback needs.
    pub const CRADLE: Self = Self(nonzero(NANOS_PER_SECOND / 15));

    /// The longest span there is: four seconds and a bit, one nanosecond short
    /// of `u32::MAX` of them.
    ///
    /// Slower than anything [`from_hz`](Self::from_hz) can name, and the bound
    /// that keeps [`Step::alpha`](crate::Step::alpha) inside a `u64`.
    pub const MAX: Self = Self(NonZeroU32::MAX);

    /// The span of exactly this many nanoseconds.
    #[must_use]
    #[inline]
    pub const fn from_nanos(nanos: NonZeroU32) -> Self {
        Self(nanos)
    }

    /// The span for `hz` ticks per second, truncated to a whole nanosecond.
    ///
    /// This is the only truncation in the crate, and what it costs -- a
    /// simulation running fast against a wall clock by `1_000_000_000 % hz`
    /// nanoseconds per second -- is tabulated rate by rate in the [crate
    /// documentation](crate).
    ///
    /// Rates above a gigahertz truncate to nothing, so the span floors at one
    /// nanosecond and [`Step`](crate::Step) keeps a divisor it can divide by.
    #[must_use]
    #[inline]
    pub const fn from_hz(hz: NonZeroU32) -> Self {
        Self(nonzero(NANOS_PER_SECOND / hz.get()))
    }

    /// The span of exactly this many whole milliseconds.
    ///
    /// The constructor a game writes. It is total because zero milliseconds is
    /// not a span: a `0` is taken as the shortest span there is, the same
    /// answer every other zero in this module gets, rather than as a division
    /// by zero in [`Step`](crate::Step).
    ///
    /// A game wanting a span no whole millisecond names -- a 72 Hz headset's
    /// 13 888 888 ns -- has [`from_nanos`](Self::from_nanos).
    ///
    /// ```
    /// use core::time::Duration;
    /// use corvid_time::TickSpan;
    ///
    /// const THIRTY_HZ: TickSpan = TickSpan::from_millis(33);
    /// assert_eq!(THIRTY_HZ.period(), Duration::from_millis(33));
    /// assert_eq!(THIRTY_HZ.hz(), 30);
    ///
    /// // Exact across the whole range.
    /// assert_eq!(TickSpan::from_millis(1).period(), Duration::from_millis(1));
    /// assert_eq!(TickSpan::from_millis(255).period(), Duration::from_millis(255));
    /// ```
    #[must_use]
    #[inline]
    #[allow(
        clippy::cast_lossless,
        reason = "`u32::from` is not a `const fn` and this has to be one, so that a span is available where a constant is required; widening a `u8` cannot lose anything, which is the whole of what the lint is guarding"
    )]
    pub const fn from_millis(millis: u8) -> Self {
        Self(nonzero(millis as u32 * 1_000_000))
    }

    /// How long a tick lasts, in nanoseconds.
    #[must_use]
    #[inline]
    pub const fn nanos(self) -> u32 {
        self.0.get()
    }

    /// How long a tick lasts.
    #[must_use]
    #[inline]
    #[allow(
        clippy::cast_lossless,
        reason = "`u64::from` is not a `const fn` and this has to be one; widening a `u32` cannot lose anything, which is the whole of what the lint is guarding"
    )]
    pub const fn period(self) -> Duration {
        Duration::from_nanos(self.0.get() as u64)
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
    pub const fn hz(self) -> u32 {
        NANOS_PER_SECOND / self.0.get()
    }
}

impl Default for TickSpan {
    /// [`CRADLE`](Self::CRADLE), fifteen ticks a second.
    #[inline]
    fn default() -> Self {
        Self::CRADLE
    }
}

// No `From<NonZeroU32> for TickSpan`, and its absence is the decision: a
// non-zero `u32` is now both what a span is made of and what a rate is given
// as, so a conversion taking one would have to pick a meaning and would be read
// as the other half the time. `from_nanos`, `from_hz` and `from_millis` each say
// which they are.

/// Still known to be non-zero on the way out.
impl From<TickSpan> for NonZeroU32 {
    #[inline]
    fn from(span: TickSpan) -> Self {
        span.0
    }
}

impl From<TickSpan> for Duration {
    #[inline]
    fn from(span: TickSpan) -> Self {
        span.period()
    }
}
