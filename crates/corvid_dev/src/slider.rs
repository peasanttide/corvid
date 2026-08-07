//! The time slider, over a live or recorded session.

use core::ops::RangeInclusive;
use std::sync::Arc;

use corvid_behavior::State;
use corvid_replay::{Session, Snapshots, Unreachable};
use corvid_time::Tick;
/// Where a scrub is, and whether it is being held.
///
/// The simulation is paused while [`held`](Self::held) is set, so nothing races
/// a seek: a slider dragged over a stretch of the log warms the snapshot ring
/// and the run resumes from wherever it was let go of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Slider {
    /// The tick the scrub is on.
    pub at: Tick,
    /// Whether it is being dragged.
    pub held: bool,
}

impl Slider {
    /// A slider parked at `at`.
    #[must_use]
    #[inline]
    pub const fn new(at: Tick) -> Self {
        Self { at, held: false }
    }

    /// Where the slider may go: the whole of what the session still holds.
    ///
    /// A run that has forgotten its far past opens at the oldest tick it kept,
    /// so this is the reach of a seek rather than the age of the game.
    #[must_use]
    pub fn range<S: State>(session: &Session<S>) -> RangeInclusive<Tick> {
        session.first()..=session.last()
    }

    /// Scrub.
    ///
    /// One call of [`Session::seek`], with no second implementation: a slider
    /// that re-simulated its own way to a tick would be a replay to keep in
    /// step with the one every save, load and rollback already goes through.
    ///
    /// The state comes back as a handle, because that is what the session
    /// hands out and what the runtime is already holding: an `Opening`'s
    /// origin, a `Frame`'s two ends and a scrub's answer are all the same
    /// [`Arc`](std::sync::Arc), so parking a slider on a tick copies no state.
    /// A caller that wants the value by itself derefs.
    ///
    /// # Errors
    ///
    /// [`Unreachable`], for a tick the session's log does not cover.
    pub fn seek<S: State>(
        session: &Session<S>,
        snapshots: &mut Snapshots<S>,
        to: Tick,
    ) -> Result<(Arc<S>, u64), Unreachable> {
        session.seek(snapshots, to)
    }

    /// The scrub, clamped into what the session can reach.
    #[must_use]
    pub fn clamped<S: State>(self, session: &Session<S>) -> Tick {
        self.at.clamp(session.first(), session.last())
    }
}

impl From<Tick> for Slider {
    #[inline]
    fn from(at: Tick) -> Self {
        Self::new(at)
    }
}

impl From<Slider> for Tick {
    #[inline]
    fn from(slider: Slider) -> Self {
        slider.at
    }
}
