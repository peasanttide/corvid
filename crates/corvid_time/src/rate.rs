//! How fast the simulation runs, and the exact period that follows from it.

use core::num::NonZeroU32;
use core::time::Duration;

/// Nanoseconds in a second, which is the numerator every period comes from.
const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// Builds a [`NonZeroU32`] in a `const` context without an `unwrap`.
///
/// `NonZeroU32::new` returns an `Option`, and unwrapping one is denied by the
/// workspace lints for the same reason it is denied everywhere else. A zero
/// rate has no honest period, so the fallback is the slowest rate there is.
const fn nonzero(hz: u32) -> NonZeroU32 {
    match NonZeroU32::new(hz) {
        Some(hz) => hz,
        None => NonZeroU32::MIN,
    }
}

/// How many ticks the simulation runs per second.
///
/// The rate is part of a session's opening rather than a runtime setting: two
/// peers at different rates are two different simulations, and a replay
/// recorded at one rate does not mean anything at another. That is why this is
/// a hashable value with a wire format and not a number in a config struct.
///
/// ```
/// use core::num::NonZeroU32;
/// use core::time::Duration;
/// use corvid_time::TickRate;
///
/// // A zero rate is a division by zero, so the type refuses to hold one.
/// const FAST: TickRate = match NonZeroU32::new(60) {
///     Some(hz) => TickRate::from_hz(hz),
///     None => TickRate::CRADLE,
/// };
///
/// assert_eq!(TickRate::CRADLE.hz(), 15);
/// assert_eq!(TickRate::CRADLE.period(), Duration::from_nanos(66_666_666));
/// assert_eq!(FAST.period(), Duration::from_nanos(16_666_666));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(::serde::Serialize, ::serde::Deserialize),
    serde(transparent)
)]
pub struct TickRate(NonZeroU32);

impl TickRate {
    /// Fifteen ticks a second: the rate Corvid's own example runs at, and the
    /// default everything else inherits.
    ///
    /// Fifteen is low on purpose. A tick has sixty-six milliseconds to simulate
    /// in, which is room for a rollback of half a dozen ticks over fifty
    /// thousand entities inside one frame's budget, and the display is not
    /// waiting on any of it — the camera and the cursor run at the refresh rate
    /// and never ask the simulation for permission. Raising the rate buys
    /// nothing the player can see and spends the headroom rollback needs.
    pub const CRADLE: Self = Self(nonzero(15));

    /// The rate for `hz` ticks per second.
    #[must_use]
    #[inline]
    pub const fn from_hz(hz: NonZeroU32) -> Self {
        Self(hz)
    }

    /// How many ticks run per second.
    #[must_use]
    #[inline]
    pub const fn hz(self) -> u32 {
        self.0.get()
    }

    /// One tick's period, as a whole number of nanoseconds.
    ///
    /// The division truncates, and that is the definition rather than an
    /// approximation of one: the step accumulates against this same integer, so
    /// advancing by exactly this many nanoseconds a thousand times delivers
    /// exactly a thousand ticks with nothing left over. A period carrying a
    /// remainder would leave the two disagreeing, and a replay driven by its
    /// own rate's period would come up one tick short of the run it recorded.
    ///
    /// What truncation costs is that the simulation runs fast against real
    /// time, by exactly `1_000_000_000 % hz` nanoseconds per second — ten of
    /// them at fifteen hertz, which is one second of drift every three years,
    /// against a wall clock nobody is comparing the tick counter to anyway.
    ///
    /// Rates above a gigahertz would truncate to nothing, so the period clamps
    /// at one nanosecond and the step keeps a denominator it can divide by.
    #[must_use]
    #[inline]
    #[allow(
        clippy::cast_lossless,
        reason = "`u64::from` is not a `const fn` and this has to be one, so that a period is available where a constant is required; widening a `u32` cannot lose anything, which is the whole of what the lint is guarding"
    )]
    pub const fn period_nanos(self) -> u64 {
        let nanos = NANOS_PER_SECOND / self.0.get() as u64;
        if nanos == 0 { 1 } else { nanos }
    }

    /// One tick's period.
    ///
    /// Exactly [`period_nanos`](Self::period_nanos) nanoseconds; see there for
    /// why it is a truncated integer and what that costs.
    #[must_use]
    #[inline]
    pub const fn period(self) -> Duration {
        Duration::from_nanos(self.period_nanos())
    }
}

impl Default for TickRate {
    /// [`CRADLE`](Self::CRADLE), fifteen ticks a second.
    #[inline]
    fn default() -> Self {
        Self::CRADLE
    }
}

/// The rate for that many ticks a second, which is
/// [`from_hz`](TickRate::from_hz).
///
/// It takes the non-zero type rather than a `u32` because a zero rate has no
/// period, and the place to refuse one is the type rather than a fallible
/// conversion every caller then has to unwrap.
impl From<NonZeroU32> for TickRate {
    #[inline]
    fn from(hz: NonZeroU32) -> Self {
        Self::from_hz(hz)
    }
}

/// How many ticks run per second, still known to be non-zero.
impl From<TickRate> for NonZeroU32 {
    #[inline]
    fn from(rate: TickRate) -> Self {
        rate.0
    }
}

/// How many ticks run per second, which is [`hz`](TickRate::hz).
impl From<TickRate> for u32 {
    #[inline]
    fn from(rate: TickRate) -> Self {
        rate.hz()
    }
}
