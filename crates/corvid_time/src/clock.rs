//! Where real time gets in, and the one clock that lets it.

use core::mem;
use core::time::Duration;

/// A source of elapsed real time.
///
/// The simulation is never handed one. Nothing offers a game the time, so a
/// game that wants it has to go looking — and a simulation that read a clock
/// would produce a different state on a slower machine, so every save, replay
/// and peer would disagree.
///
/// That is a narrowing and not a barrier, and it is worth saying so here rather
/// than leaving a reader to believe a signature settles it. Nothing stops a
/// simulation calling `SystemTime::now()`; what a game does about that is keep
/// its simulation crate free of anything that can, and check its ticks against
/// each other.
///
/// This trait exists one level out, for the loop that drives the simulation. It
/// is what lets that loop be handed a [`Clock`] in either mode, and it is why a
/// headless run of ten thousand ticks finishes as fast as the processor manages
/// rather than in eleven minutes.
///
/// # Implementing one
///
/// [`elapsed`](Elapsed::elapsed) returns the time since the previous call, not
/// a timestamp — the loop wants an interval, and an implementation that has to
/// subtract two absolute times is the one place a clock going backwards can
/// turn into a negative interval. Returning an interval directly means the
/// answer is unsigned all the way through.
///
/// [`Clock`] is the implementation this crate ships and the only one a game
/// needs. The trait stays because a test that wants a clock which stalls, or
/// jumps, or answers from a script is writing a few lines rather than asking
/// for a mode nobody else wants.
///
/// It is [`Debug`] because the loop holds one behind a `Box<dyn Elapsed>` and
/// derives its own, and a trait object prints only what its trait allows.
pub trait Elapsed: core::fmt::Debug {
    /// The time that has passed since the last call.
    ///
    /// The first call measures from whenever the clock was created.
    fn elapsed(&mut self) -> Duration;
}

/// Which of the two things a [`Clock`] is doing.
///
/// Private, because the choice is made by a constructor and never changed
/// afterwards: a clock that switched from the wall to a fixed step mid-run
/// would hand the loop one interval measured against real time and the next
/// against nothing, and no caller has ever wanted that.
// No `Hash`, for the reason `Clock` has none: the wall variant holds an
// `Instant`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Mode {
    /// Hands out a fixed step per call, plus whatever
    /// [`advance`](Clock::advance) queued.
    Stepped {
        /// Handed out on every call.
        step: Duration,
        /// Handed out once, on the next call.
        queued: Duration,
    },
    /// Reads the operating system's monotonic clock.
    #[cfg(feature = "std")]
    Wall {
        /// When [`elapsed`](Elapsed::elapsed) last answered.
        last: std::time::Instant,
    },
}

/// The clock, in either of the two modes anything here needs.
///
/// # One type rather than two
///
/// A separate fake clock and wall clock would be one type wearing two names:
/// both answer one question, both are consumed by being read, and every caller
/// that held one would hold it behind the same `Box<dyn Elapsed>` anyway. Two
/// types would mean two constructors to find, two `Debug` impls, two entries in
/// every import line, and a test that wanted to swap real time for a fixed step
/// changing a type rather than a call.
///
/// [`stepping`](Self::stepping) is what a headless test wants: one period per
/// call, forever, so the loop it drives ticks exactly once per iteration and a
/// test about the thousandth tick is about the thousandth tick rather than
/// about how long the machine took to get there. It still takes a nudge —
/// [`advance`](Self::advance) adds to the next reading either way.
///
/// [`still`](Self::still) with [`advance`](Self::advance) is the other case —
/// handing the loop an irregular sequence of frame times on purpose, to test
/// what it does with a long one.
///
/// [`wall`](Self::wall) is what a game's `main` builds, and the only mode that
/// talks to the world. It is monotonic rather than calendar time, so nothing
/// moves when the system clock is set backwards, and an interval that would
/// somehow measure negative saturates to zero rather than panicking.
///
/// ```
/// use core::time::Duration;
/// use corvid_time::{Clock, Elapsed, Step, TickSpan};
///
/// let span = TickSpan::CRADLE;
/// let mut clock = Clock::stepping(span.period());
/// let mut step = Step::new(span);
///
/// for _ in 0..1000 {
///     assert_eq!(step.advance(clock.elapsed()), 1);
/// }
/// assert_eq!(step.dropped(), 0);
/// ```
///
/// # Not `Copy`
///
/// For the same reason [`Step`] is not: a clock is consumed by reading it, and
/// a copy that gets read is time the original hands out a second time. Passing
/// one by value to a helper and then reading the original would silently
/// deliver the same queued interval twice, which is a doubled tick count in a
/// test whose whole job is to be exact about tick counts. In the wall mode the
/// copy carries the old mark and measures from the original's past.
///
/// `Clone` stays, because snapshotting a clock to replay a frame is a real
/// thing to want and `clone` says at the call site that a second copy of the
/// time now exists.
///
/// [`Step`]: crate::Step
// No `Hash`, and that is the derive doing what the documentation promises
// rather than the documentation asking. `corvid_hash::digest` takes anything
// that implements `Hash`, so a derived one here would have made
// `digest(&Clock::wall())` compile — a reading of this machine's monotonic
// clock, absorbed into a value two machines compare. It answered differently
// on two calls a millisecond apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clock {
    /// Which of the two this is.
    mode: Mode,
}

impl Clock {
    /// A clock that is standing still until [`advance`](Self::advance) is
    /// called.
    #[must_use]
    #[inline]
    pub const fn still() -> Self {
        Self {
            mode: Mode::Stepped {
                step: Duration::ZERO,
                queued: Duration::ZERO,
            },
        }
    }

    /// A clock that passes `period` on every call to
    /// [`elapsed`](Elapsed::elapsed), plus whatever
    /// [`advance`](Self::advance) has queued since the last one.
    #[must_use]
    #[inline]
    pub const fn stepping(period: Duration) -> Self {
        Self {
            mode: Mode::Stepped {
                step: period,
                queued: Duration::ZERO,
            },
        }
    }

    /// A clock measuring the operating system's monotonic time from now.
    #[must_use]
    #[inline]
    #[cfg(feature = "std")]
    pub fn wall() -> Self {
        Self {
            mode: Mode::Wall {
                last: std::time::Instant::now(),
            },
        }
    }

    /// Queues `by` to be added to the next [`elapsed`](Elapsed::elapsed).
    ///
    /// Calls accumulate, so two advances between two reads are one interval —
    /// which is what a real clock would have reported, and what keeps a test
    /// from having to read the clock to keep it honest.
    ///
    /// Does nothing to a [`wall`](Self::wall) clock, which measures rather than
    /// being told. That is a no-op rather than a panic because the alternative
    /// is every caller matching on a mode it chose itself two lines earlier.
    #[inline]
    pub const fn advance(&mut self, by: Duration) {
        match &mut self.mode {
            Mode::Stepped { queued, .. } => *queued = queued.saturating_add(by),
            #[cfg(feature = "std")]
            Mode::Wall { .. } => {}
        }
    }

    /// The period this clock passes on every call.
    ///
    /// Zero unless it was built by [`stepping`](Self::stepping), and zero for a
    /// [`wall`](Self::wall) clock, which has no fixed step to report.
    #[must_use]
    #[inline]
    pub const fn step(&self) -> Duration {
        match &self.mode {
            Mode::Stepped { step, .. } => *step,
            #[cfg(feature = "std")]
            Mode::Wall { .. } => Duration::ZERO,
        }
    }

    /// Whether this clock reads the world rather than being told about it.
    #[must_use]
    #[inline]
    pub const fn is_wall(&self) -> bool {
        match &self.mode {
            Mode::Stepped { .. } => false,
            #[cfg(feature = "std")]
            Mode::Wall { .. } => true,
        }
    }
}

impl Default for Clock {
    #[inline]
    fn default() -> Self {
        Self::still()
    }
}

impl Elapsed for Clock {
    #[inline]
    fn elapsed(&mut self) -> Duration {
        match &mut self.mode {
            Mode::Stepped { step, queued } => {
                let queued = mem::replace(queued, Duration::ZERO);
                step.saturating_add(queued)
            }
            #[cfg(feature = "std")]
            Mode::Wall { last } => {
                let now = std::time::Instant::now();
                let elapsed = now.saturating_duration_since(*last);
                *last = now;
                elapsed
            }
        }
    }
}
