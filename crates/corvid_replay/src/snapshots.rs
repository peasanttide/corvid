//! Whole states kept against a memory budget, spread so that a seek backwards
//! has somewhere to land.

use alloc::vec::Vec;
use core::fmt;

use corvid_behavior::State;
use corvid_time::Tick;

use crate::ActionLog;

/// A ring of whole states, sized by bytes rather than by ticks.
///
/// [`Session::seek`](crate::Session::seek) reaches a tick by restoring the
/// nearest state at or before it and re-simulating forward, so what this holds
/// decides how much re-simulation a seek costs and nothing else: emptying the
/// ring changes what a seek costs and not what it returns, which is the property
/// `tests/seek.rs` checks by running one session at a budget of one snapshot and
/// at a budget of a hundred and comparing every tick.
///
/// # A snapshot is keyed to the log that produced it
///
/// A state alone does not say which history it came from, so every entry here
/// also records the log's
/// [`generation_at`](ActionLog::generation_at) its own tick -- how many
/// corrections had landed on rows before it. [`keep`](Self::keep) is handed the
/// log so that it can, and [`nearest`](Self::nearest) is handed the log so that
/// it can skip an entry the log has since moved out from under. Without that, a
/// correction that arrived after a snapshot was kept would leave the seek that
/// landed on it returning a state the correction never touched -- a rollback that
/// silently ignores the thing it rolled back for.
///
/// This is a safety net rather than a replacement for
/// [`discard_from`](Self::discard_from). A skipped entry still costs the budget
/// until something evicts it, and a `log` field *replaced* rather than corrected
/// has no shared history to compare against at all.
///
/// # Why bytes
///
/// A tick count is the wrong unit because it does not say what it costs. Fifty
/// thousand entities is a state of about a megabyte, and a ring of two hundred
/// of those is two hundred megabytes on a machine that was told to keep two
/// hundred ticks. The budget here is the number the operator actually has, and
/// how many ticks fit in it is the answer rather than the question.
///
/// # What the budget counts, and what it does not
///
/// A state is charged the length `corvid_wire` writes it as, plus the size of
/// this ring's own entry for it. That is an estimate of what it costs and not a
/// measurement of what the allocator reserved: a `Vec` inside a state with room
/// for a thousand rows and three rows in it is charged for three. So a ring
/// asked for sixty-four mebibytes may hold rather more than that, and a state
/// whose encoding is much smaller than its footprint -- a struct-of-arrays with
/// generous capacity is exactly that shape -- is the case where the gap is
/// widest.
///
/// Charging honestly would mean asking every state how much memory it owns,
/// which is a method `State` does not have and would be one more thing a
/// game could get wrong. Measuring by encoding costs a serialization per
/// snapshot kept, which is the price paid here and is worth knowing about
/// before keeping one every tick.
///
/// # Eviction keeps a spread
///
/// The snapshots between the oldest and the newest are thinned in favour of
/// recent ticks, and those two are left alone while there is anything between
/// them to thin. That is what keeps a long seek backwards from always replaying
/// from the opening: a ring that only kept the newest states would be perfect
/// for a rollback of six ticks and useless for a slider dragged to the middle of
/// an hour-long session, which is the same function called with a different
/// argument. With nothing between them left, the oldest is what goes -- the
/// opening can stand in for it and nothing can stand in for the newest.
///
/// `tests/snapshots.rs` reads the shape that produces. Five hundred ticks
/// offered one at a time into a ring of four kibibytes leave the two newest
/// snapshots a single tick apart, the widest gap in the first half of the
/// session at least sixteen times that, a seek to five ticks behind the present
/// replaying nothing at all, and a seek to the middle of the session still
/// replaying under a quarter of what it would have from the opening. A ring of
/// that size spread evenly holds its states about twenty ticks apart, which
/// gives the last of those four and none of the first three -- the shape is what
/// those three are there to pin, rather than the ring merely being small.
///
/// # Which states retire is a property of this machine
///
/// A state that falls out of the ring is dropped, and that is the whole of it:
/// [`State`] has no hand-back for a retiring state, so nothing here needs a
/// scratch to return one into and none of the calls that let a state go takes
/// one. What this holds is its own clone rather than a handle to somebody
/// else's -- [`keep`](Self::keep) is passed a reference and copies out of it,
/// because the caller goes on simulating from the original -- so an eviction
/// really does return the memory it was charging the budget for, immediately
/// and without asking anyone.
///
/// *When* a state falls out is set by one machine's budget and by where its
/// player dragged a slider, which is the reason nothing a tick reads may be
/// downstream of it. That is the same obligation
/// [`seek`](crate::Session::seek) spells out for a `Scratch`, arriving from the
/// other direction.
pub struct Snapshots<S: State> {
    /// How many bytes of state this ring may charge itself for.
    budget: usize,
    /// How many it has charged.
    used: usize,
    /// The kept states, ordered by tick, ascending and without duplicates.
    kept: Vec<Kept<S>>,
}

/// One kept state and what it was charged.
struct Kept<S: State> {
    /// The tick this is the state *at*.
    tick: Tick,
    /// What it was charged against the budget.
    charged: usize,
    /// [`ActionLog::generation_at`] this tick, when the state was kept.
    generation: u64,
    /// The state.
    state: S,
}

impl<S: State> Snapshots<S> {
    /// A ring that may charge itself `budget` bytes.
    ///
    /// A budget of zero keeps nothing, which is legal and means every seek
    /// replays from the opening.
    #[must_use]
    pub const fn new(budget: usize) -> Self {
        Self {
            budget,
            used: 0,
            kept: Vec::new(),
        }
    }

    /// How many bytes this ring may charge itself.
    #[must_use]
    pub const fn budget(&self) -> usize {
        self.budget
    }

    /// How many it has charged.
    #[must_use]
    pub const fn charged(&self) -> usize {
        self.used
    }

    /// How many states it is holding.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.kept.len()
    }

    /// Whether it is holding none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }

    /// The ticks it is holding, oldest first.
    pub fn ticks(&self) -> impl Iterator<Item = Tick> + '_ {
        self.kept.iter().map(|kept| kept.tick)
    }

    /// The latest state at or before `tick` that `log` could still have
    /// produced, if there is one.
    ///
    /// The log is an argument rather than absent because an entry kept before a
    /// correction to a row the entry's own state was built from is a state of a
    /// history that did not happen. Such an entry is skipped here and the search
    /// carries on backwards, which is what makes a rollback land on the newest
    /// snapshot the correction did *not* invalidate rather than on the opening.
    ///
    /// An entry from before the log's [`first`](ActionLog::first) is skipped for
    /// a second reason: the rows it would be replayed over are gone, so a seek
    /// starting there re-simulates the forgotten ticks against
    /// [`Action::default`](Default::default) and answers a state of a history
    /// that did not happen. The generation guard alone does not catch it --
    /// `generation_at` reports zero for every tick at or before `first`, which
    /// is exactly what an entry kept before a
    /// [`forget_before`](crate::Session::forget_before) recorded -- so the
    /// opening is the floor here as well as the ceiling.
    #[must_use]
    pub fn nearest(&self, log: &ActionLog<S::Action>, tick: Tick) -> Option<(Tick, &S)> {
        self.kept
            .iter()
            .rev()
            .find(|kept| {
                kept.tick <= tick
                    && kept.tick >= log.first()
                    && kept.generation == log.generation_at(kept.tick)
            })
            .map(|kept| (kept.tick, &kept.state))
    }

    /// Offers a state for keeping, and says whether it is being kept.
    ///
    /// The state has to be one simulated against `log`, whose
    /// [`generation_at`](ActionLog::generation_at) `tick` is recorded alongside
    /// it so that a later correction to a row before `tick` takes this entry out
    /// of [`nearest`](Self::nearest)'s reach.
    ///
    /// A state already kept for `tick` is replaced, because a rollback reaches
    /// the same tick again with a state built from a corrected log and the old
    /// one is about a history that no longer happened.
    ///
    /// `false` means the ring is not holding this state when the call returns,
    /// which happens when the state alone does not fit the budget, when it
    /// could not be encoded and so could not be charged, or when it was evicted
    /// again immediately in favour of what was already here. None of the three
    /// is an error: a ring that keeps nothing is a slow seek and not a wrong
    /// one.
    ///
    /// The state is taken by reference and cloned, because the caller keeps
    /// simulating from it. Anything this evicts to make room is dropped.
    pub fn keep(&mut self, log: &ActionLog<S::Action>, tick: Tick, state: &S) -> bool {
        let Ok(bytes) = corvid_wire::encode(state) else {
            return false;
        };
        let charged = bytes.len().saturating_add(size_of::<Kept<S>>());
        if charged > self.budget {
            return false;
        }

        let at = match self.kept.binary_search_by_key(&tick, |kept| kept.tick) {
            Ok(at) => {
                // The state that was here goes with it, at the end of this
                // arm: a rollback has reached this tick again with one built
                // from a corrected log, and the old one is of a history that
                // no longer happened.
                let old = self.kept.remove(at);
                self.used = self.used.saturating_sub(old.charged);
                at
            }
            Err(at) => at,
        };
        self.kept.insert(
            at,
            Kept {
                tick,
                charged,
                generation: log.generation_at(tick),
                state: state.clone(),
            },
        );
        self.used = self.used.saturating_add(charged);

        while self.used > self.budget {
            let Some(evicted) = self.evict() else {
                break;
            };
            if evicted == tick {
                return false;
            }
        }
        true
    }

    /// Drops every kept state at or after `tick`.
    ///
    /// This is what a correction to the log makes worth doing, and it is the
    /// counterpart of
    /// [`HashTrace::truncate_from`](crate::HashTrace::truncate_from). It is no
    /// longer what keeps a correction out of an answer -- every entry records the
    /// generation it was taken under and [`nearest`](Self::nearest) skips the
    /// ones a correction has invalidated -- but a skipped entry is still charged
    /// against the budget and still crowds out a state the ring could use. This
    /// is what gives that budget back, and the memory with it.
    ///
    /// Which ones are worth dropping is worth being exact about. The row at tick
    /// `T` carries the state at `T` to the state at `T + 1`, so a correction for
    /// tick `T` leaves the state *at* `T` untouched and invalidates every state
    /// after it: the tight call is `discard_from(T.next())`. Passing `T` throws
    /// away one snapshot that was still good and is the version that cannot be
    /// got wrong -- the same distinction
    /// [`ActionLog::generation_at`](crate::ActionLog::generation_at) draws, in
    /// the one place where a caller has to draw it by hand.
    ///
    /// It is also the tool for the case the generation cannot see: a `Session`
    /// whose [`log`](crate::Session::log) is *replaced* rather than corrected.
    /// Two logs built separately share no history to compare, so the ring has to
    /// be told, and [`clear`](Self::clear) is the version of that which needs no
    /// tick.
    pub fn discard_from(&mut self, tick: Tick) {
        while self.kept.last().is_some_and(|kept| kept.tick >= tick) {
            if let Some(kept) = self.kept.pop() {
                self.used = self.used.saturating_sub(kept.charged);
            }
        }
    }

    /// Drops every kept state.
    pub fn clear(&mut self) {
        self.kept.clear();
        self.used = 0;
    }

    /// Evicts one state and returns the tick it was at.
    ///
    /// The candidate is the one whose removal costs least, where the cost of
    /// removing a snapshot is how far a seek would then have to replay across
    /// the gap it leaves, discounted by how old it is. Removing the entry at
    /// index `i` merges the interval that ends at it into the one after, so the
    /// gap becomes `tick[i + 1] - tick[i - 1]`; dividing by the entry's age
    /// makes a small gap near the present expensive to lose and a small gap far
    /// in the past cheap, which is what turns an even spread into one that
    /// thickens towards the tick the session is actually on.
    ///
    /// The oldest and the newest are not candidates. The newest because the
    /// forward path always wants it, and the oldest because the whole point of
    /// a spread is that something old survives -- without that rule a ring under
    /// pressure collapses onto the last few ticks and a seek to the middle of
    /// the session replays from the opening every time.
    fn evict(&mut self) -> Option<Tick> {
        let newest = self.kept.last()?.tick;
        let mut choice = None;
        for index in 1..self.kept.len().saturating_sub(1) {
            let (Some(before), Some(entry), Some(after)) = (
                self.kept.get(index - 1),
                self.kept.get(index),
                self.kept.get(index + 1),
            ) else {
                continue;
            };
            let gap = u128::from(after.tick.since(before.tick));
            let age = u128::from(newest.since(entry.tick)).saturating_add(1);
            let score = (gap << 32) / age;
            if choice.is_none_or(|(_, best)| score < best) {
                choice = Some((index, score));
            }
        }

        // With fewer than three kept states there is no interior to thin, and
        // something still has to go or the loop that calls this would not end.
        // The oldest is the one to lose: the opening is always available to
        // replay from and the newest is what the forward path is about to ask
        // for.
        let index = choice.map_or(0, |(index, _)| index);
        if index >= self.kept.len() {
            return None;
        }
        let evicted = self.kept.remove(index);
        self.used = self.used.saturating_sub(evicted.charged);
        Some(evicted.tick)
    }
}

impl<S: State> fmt::Debug for Snapshots<S> {
    /// The shape rather than the states. A ring at a realistic budget holds a
    /// hundred megabytes of struct-of-arrays, and a `Debug` that printed it
    /// would be unreadable in the one place it gets called from, which is a
    /// failing assertion.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ticks: Vec<u64> = self.kept.iter().map(|kept| kept.tick.0).collect();
        f.debug_struct("Snapshots")
            .field("budget", &self.budget)
            .field("charged", &self.used)
            .field("ticks", &ticks)
            .finish()
    }
}
