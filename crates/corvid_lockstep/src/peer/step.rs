//! Re-simulating: the rollback, the one tick, and the desync report.
//!
//! The seam is that nothing here talks to anybody. These are the private steps
//! the three files beside this one reach for once they have decided something
//! has to be recomputed.

#[cfg(feature = "dev")]
use alloc::vec::Vec;

use corvid_behavior::{PlayerId, State};
use corvid_hash::{Digest, digest};
use corvid_time::Tick;

#[cfg(feature = "dev")]
use crate::Desync;
use crate::{Halt, Peer, Rolled, predict::row_at, rollback::step};

impl<S: State> Peer<S> {
    /// The rule, stated once so that no call site has to restate it.
    ///
    /// The state *at* `at` is the result of simulating the rows *before* `at`,
    /// so a correction to the row at `at` does not invalidate it: the ring is
    /// told to discard from `at.next()` and the snapshot at `at` is what the
    /// re-simulation starts from. Passing `at` would not be the cautious
    /// version of that -- forward play keeps the state at `S` before row `S` is
    /// written, so every entry the ring ever holds would go and every rollback
    /// would replay from the opening.
    pub(super) fn roll_back(&mut self, at: Tick) -> Result<Rolled, Halt> {
        let was = self.tick;
        if at >= was {
            return Ok(Rolled {
                from: at,
                to: was,
                ticks: 0,
            });
        }

        self.snapshots.discard_from(at.next());

        let ceiling = at.saturating_add(u64::from(self.budget.rollback));
        let target = if was > ceiling { ceiling } else { was };

        let (from, restored) = self.restore(at)?;
        self.state = restored;
        self.tick = from;
        self.session.marks.truncate_from(from.next());

        while self.tick < target {
            self.simulate_one(&mut corvid_behavior::Discard::new());
        }

        if self.resume < was {
            self.resume = was;
        }
        self.depth = u8::try_from(was.since(at)).unwrap_or(u8::MAX);
        Ok(Rolled {
            from: at,
            to: self.tick,
            ticks: u8::try_from(self.tick.since(at)).unwrap_or(u8::MAX),
        })
    }

    /// One tick forward from wherever this peer is, against the row prediction
    /// makes.
    pub(super) fn simulate_one(&mut self, command: &mut impl corvid_behavior::Command) {
        row_at(&self.session.log, &self.frontier, self.tick, &mut self.row);
        // Whether this is the first time this tick has been simulated, read
        // before the tick moves. `reached` is the high-water mark rather than
        // `tick`, because a rollback puts `tick` back and the ticks it replays
        // are ticks this peer has already been through.
        let fresh = self.tick >= self.reached;
        // The rule, as a choice of sink rather than as a `Vec` filtered after
        // the fact: a tick simulated for the first time may ask the runtime for
        // things, and a tick being replayed to work off a rollback may not.
        // A tick that asked to quit, to save, or to rumble a pad asked once.
        self.state = if fresh {
            step::<S>(&self.session, &self.state, self.tick, &self.row, command)
        } else {
            step::<S>(
                &self.session,
                &self.state,
                self.tick,
                &self.row,
                &mut corvid_behavior::Discard::new(),
            )
        };
        self.tick = self.tick.next();
        if self.tick > self.reached {
            self.reached = self.tick;
        }
        self.session.marks.truncate_from(self.tick);
        self.session.marks.push(digest(&self.state));
        self.snapshots
            .keep(&self.session.log, self.tick, &self.state);
    }

    /// Compares a mark that arrived, when there is anything final to compare it
    /// against.
    ///
    /// A mark for a tick past [`Frontier::agreed`](crate::Frontier::agreed) is about a state one of the
    /// two peers predicted part of, so a disagreement there is a packet in
    /// flight rather than a divergence. The marks that matter arrive a moment
    /// later, for ticks both peers have confirmed.
    pub(super) fn check_mark(
        &mut self,
        seat: PlayerId,
        at: Tick,
        mark: Digest,
    ) -> Result<(), Halt> {
        if at > self.frontier.agreed() {
            return Ok(());
        }
        self.blamed = seat;
        self.compare(seat, at, mark)?;
        if self.session.marks.get(at).is_some() && at > self.agreed_marks {
            self.agreed_marks = at;
        }
        Ok(())
    }

    /// A report about this peer, for the bisector to fill in.
    #[cfg(feature = "dev")]
    pub(crate) fn desync_at(
        &self,
        at: Tick,
        fields: Vec<crate::FieldReport>,
        first_divergent: Option<crate::Where>,
    ) -> Desync {
        let local = self.session.marks.get(at).unwrap_or_default();
        Desync {
            at,
            peer: self.blamed,
            agreed_through: self.agreed_marks,
            local,
            remote: local,
            fields,
            first_divergent,
        }
    }
}
