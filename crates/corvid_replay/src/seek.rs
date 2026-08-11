//! The one function save, load, replay, rollback and time-walk are all made of.

use alloc::{sync::Arc, vec::Vec};
use core::fmt;

use corvid_behavior::{Discard, PlayerId, PlayerState, State};
use corvid_time::Tick;

use crate::{Session, Snapshots};

impl<S: State> Session<S> {
    /// Reaches any tick the log covers: restore the nearest snapshot at or
    /// before `to`, then re-simulate forward against the log.
    ///
    /// This one function is load, replay, rollback and time-walk. A save writes
    /// the session down; loading it is a seek to its last tick. A rollback is a
    /// seek backwards after [`ActionLog::set`](crate::ActionLog::set) has taken
    /// a correction. A slider in a dev console is a seek per frame. None of
    /// them is a separate code path, which is the point: a bug in one of the
    /// five is a bug in all five and shows up in whichever is tested.
    ///
    /// The snapshot ring is a cache. What it holds decides how many ticks this
    /// re-simulates and not what it returns: the same session at a budget of one
    /// snapshot and of a hundred gives the same state for every tick, and
    /// `tests/seek.rs` is where that is checked rather than claimed. The state
    /// this arrives at is offered to the ring on the way out, so a slider
    /// dragged back and forth over one stretch warms it.
    ///
    /// Commands the re-simulated ticks return are dropped. They were requests
    /// to the platform made when those ticks first ran, and a replay that
    /// re-issued them would save a file, take a screenshot or quit for a second
    /// time.
    ///
    /// # What a game owes for the result to mean anything
    ///
    /// This is the call site that makes `corvid_behavior`'s obligations
    /// load-bearing, so it is worth naming which ones and not restating them:
    /// each is stated once, there, and linked here.
    ///
    /// **A clone has to be a copy**, because a snapshot is a clone and so is
    /// every state restored out of the ring -- see
    /// [`Data`](corvid_behavior::Data).
    ///
    /// That is the whole of what a seek asks of a game, and it is worth saying
    /// why there is nothing else: a seek re-simulates from whichever snapshot
    /// survived one machine's memory budget, so any channel into a tick that
    /// accumulated across ticks would answer differently here than it did when
    /// the session ran -- from the same log, on the same machine. A tick takes
    /// `self` by value and has no such channel, so a replay cannot violate the
    /// contract on its own.
    ///
    /// # How much of the session it had to run again
    ///
    /// The second half of the answer is how many ticks were re-simulated to
    /// reach `to`. That is the only evidence the snapshot ring did any good:
    /// the *state* a seek returns is the same whatever snapshot it started
    /// from -- which is the property `tests/seek.rs` exists to check -- so
    /// nothing about the state could ever say whether the ring was consulted.
    ///
    /// Counting it here rather than in the game is what keeps it out of the
    /// hashed state: an odometer a tick incremented would be a column on the
    /// wire that exists for a measurement.
    ///
    /// # What comes back, and what it costs
    ///
    /// An [`Arc<S>`](Arc), because that is what the callers of this hold:
    /// a runtime keeps its two states behind handles so that it can hand one
    /// to the client-local half without copying it, and a state handed back by
    /// value would be wrapped the moment it arrived.
    ///
    /// The re-simulated stretch in between is *not* held that way. Each
    /// intermediate state is produced by value, wrapped, and dropped one
    /// iteration later when the next replaces it, which costs one refcount
    /// header per tick against a whole state built per tick. The two ends are
    /// where the shape pays for itself: a seek to
    /// [`first`](crate::Session::first) over an empty ring re-simulates nothing and
    /// returns the opening's own handle, and a seek that lands exactly on a
    /// snapshot copies the ring's state once and no more.
    ///
    /// # What a correction does to the ring
    ///
    /// When [`ActionLog::set`](crate::ActionLog::set) takes a correction for
    /// tick `T`, every snapshot *after* `T` describes a history that no longer
    /// happened. The state at `T` is what the rows before `T` produce and the
    /// row at `T` is what carries it on to `T + 1`, so the state at `T` itself
    /// is untouched. A seek that landed on one of the later ones would return it
    /// without re-simulating, leaving the correction in the log and out of the
    /// answer.
    ///
    /// This is what the log's generation is for, and it is checked here rather
    /// than remembered by a caller. Each entry in the ring records
    /// [`ActionLog::generation_at`](crate::ActionLog::generation_at) its own
    /// tick, so `T`'s correction takes every entry after `T` out of reach and
    /// leaves the ones at and before it alone -- which is what keeps a rollback
    /// from replaying the session from its opening every time a packet arrives
    /// late. Taking the entry at `T` as well would not be a cautious version of
    /// that rule: forward play keeps the state at `S` before row `S` is
    /// written, so it would take every entry the ring ever holds, and
    /// `ordinary_play_does_not_invalidate_the_snapshot_it_has_just_kept` is the
    /// test that fails against it.
    ///
    /// Two things it does not do. A skipped entry still costs the budget until
    /// [`Snapshots::discard_from`] takes it back, which is why that call is
    /// still worth making after a rollback and is still the counterpart of
    /// [`HashTrace::truncate_from`](crate::HashTrace::truncate_from). And a
    /// [`log`](crate::Session::log) *replaced* wholesale rather than corrected shares
    /// no history with the one the ring was filled from, so nothing here can
    /// compare them: that case is [`Snapshots::clear`].
    ///
    /// # What this does not reproduce
    ///
    /// **Nothing about the inputs, which is why [`PlayerState`] has three fields.**
    /// Every one of them comes from the session: the seat is the roster's order,
    /// the [`Presence`](corvid_behavior::Presence) is
    /// [`Profile::presence_at`](crate::Profile::presence_at) of the roster's
    /// join and leave ticks, and the action is the log's. This function is what
    /// rules a head-and-hands pose out of that struct: the log records actions
    /// and not poses, so there would be nothing here to rebuild one from and
    /// every player would be handed the identity, which makes a game that read
    /// it replay to a different state than it ran. [`PlayerState`] says so at
    /// length.
    ///
    /// **A session whose parts were put out of step by hand.** The roster
    /// decides who is seated and the log decides what they did, and this does
    /// not compare them: a seat the log has no column for reads
    /// `Action::default()`, and a row wider than the roster has its extra
    /// columns ignored. That is a replay of a session that never happened rather
    /// than an error. [`load`](Self::load) refuses a *capture* like that, and
    /// [`check`](Self::check) is the same comparison as a call for a session
    /// that was assembled rather than decoded -- which is as far as it can go,
    /// since the three fields are public and an assignment to one of them is
    /// not something a type can observe.
    ///
    /// # Errors
    ///
    /// [`Unreachable::Before`] for a tick before the opening, and
    /// [`Unreachable::After`] for one the log has no rows to reach.
    pub fn seek(
        &self,
        snapshots: &mut Snapshots<S>,
        to: Tick,
    ) -> Result<(Arc<S>, u64), Unreachable> {
        let first = self.opening.first;
        if to < first {
            return Err(Unreachable::Before { to, first });
        }
        let last = self.last();
        if to > last {
            return Err(Unreachable::After { to, last });
        }
        let mut resimulated = 0u64;

        let (mut at, mut state) = snapshots.nearest(&self.log, to).map_or_else(
            || (first, self.opening.origin()),
            |(tick, state)| (tick, Arc::new(state.clone())),
        );

        // The action a seat with no column gets, and the one every seat gets on
        // a tick the log does not cover. It is a binding rather than a
        // temporary because the roster below borrows it.
        let idle = S::Action::default();
        let mut roster: Vec<PlayerState<S::Action>> = Vec::new();

        while at < to {
            roster.clear();
            for (seat, profile) in self.opening.roster.iter().enumerate() {
                // A roster longer than a `PlayerId` can address is refused by
                // both `new` and `check`, so this stops rather than folding the
                // seats past the end onto the last addressable one: a session
                // that dodged both checks is missing those seats, and a second
                // copy of seat 65 535's action would be a session that never
                // happened rather than one that is short a player.
                let Ok(seat) = u16::try_from(seat) else {
                    break;
                };
                let id = PlayerId(seat);
                let Some(presence) = profile.presence_at(at) else {
                    continue;
                };
                roster.push(PlayerState {
                    id,
                    presence,
                    action: self
                        .log
                        .get(at, id)
                        .cloned()
                        .unwrap_or_else(|| idle.clone()),
                });
            }

            // Requests these ticks made are dropped: they were made when the
            // ticks first ran, and a replay that re-issued them would save a
            // file, take a screenshot or quit for a second time.
            let next = S::clone(&state).tick(
                &self.opening.content,
                &roster,
                &self.opening.rules,
                &mut Discard::new(),
            );
            // The handle this replaces is the state one tick back. Nothing here
            // is the last holder of it as a rule -- the ring, a frame, or a
            // caller's rollback buffer may be holding the same value -- so the
            // assignment is a decrement and only sometimes a free.
            state = Arc::new(next);
            at = at.next();
            resimulated += 1;
        }

        snapshots.keep(&self.log, to, &state);
        Ok((state, resimulated))
    }
}

/// A tick is not in the session.
///
/// Both cases are about the log's extent and neither is about the snapshot
/// ring, which is the distinction worth keeping: an empty ring makes a seek
/// slow and a log that stops at tick 400 makes tick 401 a tick that has not
/// been played yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Unreachable {
    /// Before the session opened. Nothing precedes the opening state.
    Before {
        /// The tick that was asked for.
        to: Tick,
        /// The tick the session opens on.
        first: Tick,
    },
    /// After the last tick the log has rows to reach.
    After {
        /// The tick that was asked for.
        to: Tick,
        /// The latest tick the log reaches.
        last: Tick,
    },
}

impl fmt::Display for Unreachable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Before { to, first } => write!(
                f,
                "tick {to} is before the session's opening tick {first}, and \
                 nothing precedes the opening state"
            ),
            Self::After { to, last } => write!(
                f,
                "tick {to} is past tick {last}, which is as far as this \
                 session's log reaches"
            ),
        }
    }
}

impl core::error::Error for Unreachable {}
