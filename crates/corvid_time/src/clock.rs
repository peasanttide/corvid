//! Where real time gets in, and the two implementations of getting it.

use core::mem;
use core::time::Duration;

/// A source of elapsed real time.
///
/// The simulation is never handed one. `tick` is a free function with no `&self`
/// and no clock among its arguments, so a game that wants the time has to go
/// looking for it rather than find it offered — a simulation that read a clock
/// would produce a different state on a slower machine, and every save, replay
/// and peer would disagree.
///
/// That is a narrowing and not a barrier, and it is worth saying so here rather
/// than leaving a reader to believe the signature settles it. Nothing in the
/// signature stops a `tick` calling `SystemTime::now()`; what a game does about
/// that is keep its simulation crate free of anything that can, and check its
/// ticks against each other. `corvid_behavior`'s `Simulate` is where that
/// obligation is written down.
///
/// This trait exists one level out, for the loop that drives the simulation. It
/// is what lets that loop be handed [`Fake`] in a test and [`Wall`] in
/// production, and it is why a headless run of ten thousand ticks finishes as
/// fast as the processor manages rather than in eleven minutes.
///
/// [`Wall`]: crate::Wall
///
/// # Implementing one
///
/// [`elapsed`](Clock::elapsed) returns the time since the previous call, not a
/// timestamp — the loop wants an interval, and an implementation that has to
/// subtract two absolute times is the one place a clock going backwards can
/// turn into a negative interval. Returning an interval directly means the
/// answer is unsigned all the way through.
/// A clock is [`Debug`] because it is held behind a `Box<dyn Clock>` in a
/// runtime that derives its own, and a trait object is only as printable as its
/// trait says it is. Every clock here is a couple of durations and a counter,
/// so there is nothing to weigh against saying so.
pub trait Clock: core::fmt::Debug {
    /// The time that has passed since the last call.
    ///
    /// The first call measures from whenever the clock was created.
    fn elapsed(&mut self) -> Duration;
}

/// A clock that passes exactly as much time as it is told to.
///
/// [`stepping`](Fake::stepping) is what a headless test wants: one period per
/// call, forever, so the loop it drives ticks exactly once per iteration and a
/// test about the thousandth tick is a test about the thousandth tick rather
/// than about how long the machine took to get there.
///
/// [`new`](Fake::new) with [`advance`](Fake::advance) is for the other case —
/// handing the loop an irregular sequence of frame times on purpose, to test
/// what it does with a long one.
///
/// ```
/// use core::time::Duration;
/// use corvid_time::{Clock, Fake, Step, TickSpan};
///
/// let rate = TickSpan::CRADLE;
/// let mut clock = Fake::stepping(rate.period());
/// let mut step = Step::new(rate);
///
/// for _ in 0..1000 {
///     assert_eq!(step.advance(clock.elapsed()), 1);
/// }
/// assert_eq!(step.dropped(), 0);
/// ```
///
/// Deliberately not `Copy`, for the same reason [`Step`] is not: a clock is
/// consumed by reading it, and a copy that gets read is time the original hands
/// out a second time. Passing one by value to a helper and then reading the
/// original would silently deliver the same queued interval twice, which is a
/// doubled tick count in a test whose whole job is to be exact about tick
/// counts. `Clone` stays, because snapshotting a clock to replay a frame is a
/// real thing to want and `clone` says at the call site that a second copy of
/// the time now exists.
///
/// [`Step`]: crate::Step
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Fake {
    /// Handed out on every call.
    step: Duration,
    /// Handed out once, on the next call.
    queued: Duration,
}

impl Fake {
    /// A clock that is standing still until [`advance`](Fake::advance) is
    /// called.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            step: Duration::ZERO,
            queued: Duration::ZERO,
        }
    }

    /// A clock that passes `period` on every call to
    /// [`elapsed`](Clock::elapsed).
    #[must_use]
    #[inline]
    pub const fn stepping(period: Duration) -> Self {
        Self {
            step: period,
            queued: Duration::ZERO,
        }
    }

    /// Queues `by` to be added to the next [`elapsed`](Clock::elapsed).
    ///
    /// Calls accumulate, so two advances between two reads are one interval —
    /// which is what a real clock would have reported, and what keeps a test
    /// from having to read the clock to keep it honest.
    #[inline]
    pub const fn advance(&mut self, by: Duration) {
        self.queued = self.queued.saturating_add(by);
    }

    /// The period this clock passes on every call, zero unless it was built by
    /// [`stepping`](Fake::stepping).
    #[must_use]
    #[inline]
    pub const fn step(&self) -> Duration {
        self.step
    }
}

impl Clock for Fake {
    #[inline]
    fn elapsed(&mut self) -> Duration {
        let queued = mem::replace(&mut self.queued, Duration::ZERO);
        self.step.saturating_add(queued)
    }
}

/// A clock that reads the operating system's monotonic time.
///
/// The one type in this crate that talks to the world, and the reason `std` is
/// a feature at all. It is a monotonic clock rather than a calendar one, so
/// nothing here moves when the system clock is set backwards, and an interval
/// that would somehow measure negative saturates to zero rather than panicking.
///
/// A game's `main` builds one of these and nothing else in the workspace ever
/// mentions it, which is the property that makes every test headless.
///
/// Not `Copy`, for the same reason [`Fake`] and [`Step`] are not. Reading this
/// clock moves its mark forward; a copy carries the old mark, so reading the
/// copy measures from the original's past and hands the loop an interval it has
/// already spent. `Clone` stays because the hazard is at least written down at
/// the call site there, and because a clone is the honest way to fork a second
/// timeline off the same instant.
///
/// [`Step`]: crate::Step
#[cfg(feature = "std")]
#[derive(Clone, Debug)]
pub struct Wall {
    /// When [`elapsed`](Clock::elapsed) last answered.
    last: std::time::Instant,
}

#[cfg(feature = "std")]
impl Wall {
    /// A clock measuring from now.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self {
            last: std::time::Instant::now(),
        }
    }
}

#[cfg(feature = "std")]
impl Default for Wall {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std")]
impl Clock for Wall {
    #[inline]
    fn elapsed(&mut self) -> Duration {
        let now = std::time::Instant::now();
        let elapsed = now.saturating_duration_since(self.last);
        self.last = now;
        elapsed
    }
}
