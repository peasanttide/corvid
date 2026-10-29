//! Keeping a seat whose machine went, and the state a save asked for.
//!
//! The seam against `exchange.rs` is that nothing here arrived from anybody.
//! A seat taken over is this machine deciding to speak for a seat it heard
//! nothing more from, and a save is this machine reading back a state it has
//! already computed: neither touches the wire.

use alloc::sync::Arc;
use alloc::vec::Vec;

use corvid_behavior::{PlayerId, SaveSlot, State};
use corvid_replay::Refused;
use corvid_time::Tick;

use super::Peer;
use crate::{predict::action_at, predict::row_at, rollback::step};

impl<S: State> Peer<S> {
    /// Speaks for `seat` from here on, as one of this machine's
    /// [`extra`](Self::extra) seats: what a host does for a guest whose
    /// machine went, so a bot plays its seat until somebody joins it again
    /// rather than the seat leaving the session for good. Answers the first
    /// tick [`submit_for`](Self::submit_for) will write for it.
    ///
    /// **The rows nobody sent are confirmed as what this machine predicted
    /// for them**: every row past the seat's last confirmed one, up to the
    /// tick before `now + delay`, holds the action prediction repeated -- the
    /// last one the seat confirmed, or the default for a seat that never
    /// spoke. That is what this machine already simulated those ticks with, so
    /// keeping a seat costs no rollback; and it is what goes out in the
    /// seat's window from now on, so every other machine confirms the same.
    ///
    /// A seat already kept, this machine's own and a seat that has departed
    /// are left as they are.
    ///
    /// # Errors
    ///
    /// [`Refused`], from the log, for a seat this session does not have or a
    /// tick it could not find the room for.
    pub fn take_over(&mut self, seat: PlayerId) -> Result<Tick, Refused> {
        let next = self.tick.saturating_add(u64::from(self.budget.delay));
        if seat == self.seat || self.extra.contains(&seat) || self.frontier.is_retired(seat) {
            return Ok(next);
        }
        let first = self.session.first();
        let from = self
            .frontier
            .confirmed(seat)
            .map_or(first, Tick::next)
            .max(first);
        if from < next {
            // One answer for every row: prediction repeats the row at the
            // seat's frontier whatever tick it is asked about.
            let repeat = action_at(&self.session.log, &self.frontier, from, seat)
                .cloned()
                .unwrap_or_default();
            self.session.log.extend_to(next.prev())?;
            let mut at = from;
            while at < next {
                if !self.session.log.is_confirmed(at, seat) {
                    self.session.log.set(at, seat, repeat.clone())?;
                }
                at = at.next();
            }
            self.frontier.observe(seat, next.prev());
        }
        self.extra.push(seat);
        Ok(next)
    }

    /// The saves final ticks have asked for since this was last asked, each
    /// with the tick that asked, oldest first. The state a save holds is the
    /// one [`final_state`](Self::final_state) answers for the tick after.
    pub fn take_saved(&mut self) -> Vec<(Tick, SaveSlot)> {
        core::mem::take(&mut self.saved)
    }

    /// The state at `at` as every machine computed it, for a tick no later
    /// than the one after the newest tick every seat has confirmed: the
    /// nearest state this peer kept at or before it, played forward over the
    /// confirmed rows between. [`None`] for a tick that is not final yet or
    /// that this peer can no longer reach.
    #[must_use]
    pub fn final_state(&self, at: Tick) -> Option<S> {
        if at > self.frontier.agreed().next() || at > self.tick {
            return None;
        }
        let (mut tick, mut state) = self.restore(at).ok()?;
        let mut row = Vec::new();
        while tick < at {
            row_at(&self.session.log, &self.frontier, tick, &mut row);
            let level = Arc::clone(self.session.levels.at(tick));
            let stepped = step::<S>(
                &self.session,
                &level,
                &state,
                tick,
                &row,
                &mut corvid_behavior::Discard::new(),
            );
            state = stepped.state;
            tick = tick.next();
        }
        Some(state)
    }
}
