//! What this machine says: its own action, and the datagram carrying it.
//!
//! The seam is direction. Everything here is outbound and reads no datagram,
//! which is what makes the rule a rollback must not break -- a peer never
//! submits twice for one tick -- statable in one file.

use corvid_behavior::{PlayerId, State};
use corvid_replay::Refused;
use corvid_time::Tick;

use crate::{Datagram, Peer};

impl<S: State> Peer<S> {
    /// Submits this machine's action, for `now + delay`.
    ///
    /// Input delay is latency traded for fewer rollbacks: an action submitted
    /// two ticks ahead usually reaches the other peers before they simulate the
    /// tick it belongs to, and a prediction that was never needed is a rollback
    /// that never happens.
    ///
    /// # Errors
    ///
    /// [`Refused`], from the log, for a seat this session does not have or a
    /// tick this machine could not find the room for. [`Refused::Confirmed`] if
    /// this machine has already submitted a different action for that tick,
    /// which is this peer contradicting itself.
    ///
    /// # A rollback does not let a peer change its mind
    ///
    /// A correction can put [`tick`](Self::tick) back by several ticks, and the
    /// naive `now + delay` would then name a tick this machine has already
    /// spoken for -- so a peer would submit a second, different action for it,
    /// send both, and be reported by everyone else as
    /// [`Halt::Contradiction`](crate::Halt::Contradiction). What this submits for instead is the first tick
    /// it has *not* spoken for, which is never behind `now + delay` and is
    /// sometimes ahead of it. The cost is that the ticks a deep rollback
    /// replayed keep the actions they were played with, which is the correct
    /// answer: they are what this player did, and the other machines have
    /// already simulated them.
    pub fn submit(&mut self, action: S::Action) -> Result<Tick, Refused> {
        let at = self.tick.saturating_add(u64::from(self.budget.delay));
        // Already spoken for. A peer that is stalled, or that a correction has
        // put back a few ticks, reaches this every tick until its own tick
        // catches up again -- and the action it is holding down now is not an
        // action for a tick it announced two seconds ago. Dropping it is the
        // only answer that keeps one story on the wire.
        if self
            .frontier
            .confirmed(self.seat)
            .is_some_and(|spoken| spoken >= at)
        {
            drop(action);
            return Ok(self.frontier.of(self.seat));
        }
        self.session.log.extend_to(at)?;
        self.session.log.set(at, self.seat, action)?;
        self.frontier.observe(self.seat, at);
        Ok(at)
    }

    /// What to send this tick.
    ///
    /// The newest [`WINDOW`](crate::WINDOW) rows of this seat's own actions,
    /// and one digest. The digest is for the newest tick this peer's state is
    /// *final* at -- its own tick or [`Frontier::agreed`](crate::Frontier::agreed), whichever is older --
    /// because a mark taken over a prediction is not a fact and comparing one
    /// would report a desync every time a packet was late.
    #[must_use]
    pub fn outgoing(&self) -> Datagram<S::Action> {
        // What this seat has actually submitted, and never a tick past it. The
        // window is read out of the log, and a row this peer has not written is
        // `Action::default()` -- so a head taken from `now + delay` alone would
        // put "idle" on the wire for a tick this machine has not decided yet,
        // every other peer would confirm it, and the real action arriving a
        // tick later would be reported as this peer contradicting itself. It is
        // reachable from an ordinary call order: `advance` moves `tick`, so
        // sending after simulating overshoots by exactly one tick.
        let want = self.tick.saturating_add(u64::from(self.budget.delay));
        let head = self.frontier.confirmed(self.seat).map_or_else(
            || self.session.first(),
            |spoken| {
                if want < spoken { want } else { spoken }
            },
        );
        let agreed = self.frontier.agreed();
        let marked = if self.tick < agreed {
            self.tick
        } else {
            agreed
        };
        let mark = self.session.marks.get(marked).unwrap_or_default();
        Datagram::build(
            &self.session.log,
            self.seat,
            head,
            self.acked(),
            self.heard_through(),
            marked,
            mark,
        )
    }

    /// The newest tick this peer has every seat's real action for, and [`None`]
    /// while any seat has said nothing at all.
    ///
    /// [`Frontier::agreed`](crate::Frontier::agreed) alone would answer the opening tick for a peer that
    /// has heard from nobody, which is a claim rather than a report -- this is
    /// the same number with the claim removed.
    pub(super) fn heard_through(&self) -> Option<Tick> {
        (0..self.frontier.seats())
            .all(|seat| self.frontier.acted(PlayerId(seat)))
            .then(|| self.frontier.agreed())
    }

    /// The newest tick every *other* seat has said it has everything through.
    ///
    /// The minimum, because one datagram goes to all of them: the seat furthest
    /// behind is the one whose missing rows have to be in it. A session with
    /// nobody else in it answers this peer's own frontier, which makes the
    /// window the minimum [`WINDOW`](crate::WINDOW) rows.
    pub(super) fn acked(&self) -> Option<Tick> {
        self.heard
            .iter()
            .enumerate()
            .filter(|(seat, _)| *seat != usize::from(self.seat.0))
            .map(|(_, heard)| *heard)
            // `None` is a seat that has acknowledged nothing, and `Option`'s own
            // order puts it below every tick -- so a seat still catching up
            // pulls the window all the way back, which is exactly what it
            // needs.
            .min()
            .unwrap_or_default()
    }
}
