//! Re-simulating: the rollback, the one tick, and the desync report.
//!
//! The seam is that nothing here talks to anybody. These are the private steps
//! the three files beside this one reach for once they have decided something
//! has to be recomputed.

#[cfg(feature = "dev")]
use alloc::vec::Vec;

use alloc::sync::Arc;
use corvid_behavior::{PlayerId, State};

use corvid_hash::Digest;
use corvid_time::Tick;

use super::held::Held;
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
        // The ticks from here are played again, and ask again for whatever
        // they change of the level.
        self.session.levels.forget_after(from);
        // What the replayed ticks asked for is asked again by the replay.
        drop(self.held.split_off(&from));

        while self.tick < target {
            self.simulate_one();
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
    ///
    /// What the tick asks the runtime for is held, over whatever an earlier
    /// simulation of it asked, until the tick is final ([`release`](Self::release)).
    /// A tick already final and handed over asks nothing again: a rollback
    /// restoring an older snapshot replays some of those.
    pub(super) fn simulate_one(&mut self) {
        row_at(&self.session.log, &self.frontier, self.tick, &mut self.row);
        let level = Arc::clone(self.session.levels.at(self.tick));
        let mut asked = Held::default();
        let stepped = if self.tick >= self.told {
            step::<S>(
                &self.session,
                &level,
                &self.state,
                self.tick,
                &self.row,
                &mut asked,
            )
        } else {
            step::<S>(
                &self.session,
                &level,
                &self.state,
                self.tick,
                &self.row,
                &mut corvid_behavior::Discard::new(),
            )
        };
        if asked.is_empty() {
            self.held.remove(&self.tick);
        } else {
            self.held.insert(self.tick, asked);
        }
        let mark = corvid_replay::mark(
            &stepped.state,
            stepped.changed.as_ref().map(|(level, _)| &**level),
        );
        if let Some((level, changes)) = stepped.changed {
            self.session.levels.push(self.tick.next(), level, changes);
        }
        self.state = stepped.state;
        self.tick = self.tick.next();
        self.session.marks.truncate_from(self.tick);
        self.session.marks.push(mark);
        self.snapshots
            .keep(&self.session.log, self.tick, &self.state);
    }

    /// Hands `sink` what every final tick asked for, oldest first: the ticks
    /// this peer has simulated whose rows every seat has confirmed. Each
    /// tick's requests go once, from its last simulation.
    pub(super) fn release(
        &mut self,
        sink: &mut impl corvid_behavior::Command<corvid_behavior::LevelEdit<S>>,
    ) {
        let settled = self.frontier.agreed().min(self.tick);
        while let Some(entry) = self.held.first_entry() {
            if *entry.key() >= settled {
                break;
            }
            entry.remove().tell(sink);
        }
        if settled > self.told {
            self.told = settled;
        }
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
