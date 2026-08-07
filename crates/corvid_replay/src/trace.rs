//! One digest per tick: what a peer compares against another peer, and what a
//! release compares against the one before it.

use alloc::vec::Vec;

use corvid_hash::Digest;
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

/// The digest of the state at each tick of a session, from `first` onwards.
///
/// The mark at tick `T` is the digest of the state *at* `T`, not of the tick
/// that produced it, so a trace that opens at `first` starts with the digest of
/// the opening state and a session that has run `n` ticks has `n + 1` marks.
///
/// Live, this is the desync detector: every peer sends its mark for tick `N`
/// alongside its action for a later one, and [`disagrees_with`](Self::disagrees_with)
/// says whether and where two peers stopped playing the same game. Recorded, it
/// is the regression detector: the same session replayed under a later build
/// either produces the same marks or names the tick it stopped doing so.
///
/// # Rollback truncates rather than overwrites
///
/// A rollback re-simulates a stretch of ticks against a corrected log, and the
/// states it produces are legitimately different from the ones it produced the
/// first time. Every mark after the tick it rolls back to is therefore about a
/// history that no longer happened, which is why the operation offered here is
/// [`truncate_from`](Self::truncate_from) and not an overwrite: a mark that
/// stayed behind would be compared against a peer that never computed it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HashTrace {
    /// The tick the first mark belongs to.
    first: Tick,
    /// One digest per tick, from `first`, contiguous.
    ///
    /// Held as the raw bits rather than as [`Digest`], which carries no serde
    /// implementation of its own — a digest is a number and this is the one
    /// place in the workspace that has to write a column of them down. The
    /// accessors are all in terms of [`Digest`], so the representation stops at
    /// this line.
    marks: Vec<u64>,
}

impl HashTrace {
    /// An empty trace for a session that opens at `first`.
    #[must_use]
    pub const fn new(first: Tick) -> Self {
        Self {
            first,
            marks: Vec::new(),
        }
    }

    /// The tick the first mark belongs to.
    #[must_use]
    pub const fn first(&self) -> Tick {
        self.first
    }

    /// How many marks are recorded.
    #[must_use]
    pub fn len(&self) -> u64 {
        u64::try_from(self.marks.len()).unwrap_or(u64::MAX)
    }

    /// Whether nothing has been marked yet.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// The tick the next [`push`](Self::push) would record at.
    #[must_use]
    pub fn end(&self) -> Tick {
        self.first.saturating_add(self.len())
    }

    /// Records a digest for [`end`](Self::end).
    pub fn push(&mut self, mark: Digest) {
        self.marks.push(mark.to_u64());
    }

    /// The mark for `tick`, if there is one.
    #[must_use]
    pub fn get(&self, tick: Tick) -> Option<Digest> {
        if tick < self.first {
            return None;
        }
        let index = usize::try_from(tick.since(self.first)).ok()?;
        self.marks.get(index).copied().map(Digest::from_u64)
    }

    /// Drops the mark for `tick` and every mark after it.
    ///
    /// A tick before [`first`](Self::first) empties the trace, because every
    /// mark it holds is after that tick.
    pub fn truncate_from(&mut self, tick: Tick) {
        if tick < self.first {
            self.marks.clear();
            return;
        }
        let keep = usize::try_from(tick.since(self.first)).unwrap_or(usize::MAX);
        self.marks.truncate(keep);
    }

    /// Drops the mark for every tick before `tick` and moves the trace's first
    /// tick to it.
    ///
    /// The other end from [`truncate_from`](Self::truncate_from), and it is
    /// there for an entirely different reason. Truncating is about a history
    /// that stopped being true, so the marks it drops were wrong; this drops
    /// marks that are perfectly good and simply older than anything is going to
    /// ask about again. A run that never seeks compares its mark for the tick it
    /// is on and keeps the rest against a rollback that reaches back seconds
    /// rather than hours, so a trace that grew for the length of a session was
    /// keeping a row per tick for nobody.
    ///
    /// [`end`](Self::end) does not move. What a trace can still be compared over
    /// is the overlap two of them have, and [`disagrees_with`](Self::disagrees_with)
    /// already reports nothing about the ticks only one of them holds — so a
    /// peer that has forgotten the first hour still detects a desync on the tick
    /// it is on, and a regression check that wants the whole column is a
    /// recorded session rather than a live one.
    pub fn forget_before(&mut self, tick: Tick) {
        if tick <= self.first {
            return;
        }
        let dropped = usize::try_from(tick.since(self.first))
            .unwrap_or(usize::MAX)
            .min(self.marks.len());
        self.marks.drain(..dropped);
        self.first = tick;
    }

    /// The first tick both traces have a mark for and disagree about.
    ///
    /// Only the overlap is compared, and that is the whole of what a mark can
    /// say. Two traces that start at different ticks agree about the ticks
    /// neither has marked and about the ticks only one of them has; a trace
    /// that is a prefix of another disagrees nowhere, because a peer that is
    /// behind is behind rather than wrong. What this reports is the earliest
    /// tick at which two peers that both computed a state computed different
    /// ones.
    ///
    /// [`None`] is therefore "nothing compared here disagreed" and not "the two
    /// sessions are the same". Two traces with no overlap at all return
    /// [`None`], which is the one answer a caller has to read carefully.
    #[must_use]
    pub fn disagrees_with(&self, other: &Self) -> Option<Tick> {
        let from = self.first.max(other.first);
        let until = self.end().min(other.end());
        let mut tick = from;
        while tick < until {
            if self.get(tick) != other.get(tick) {
                return Some(tick);
            }
            tick = tick.next();
        }
        None
    }
}
