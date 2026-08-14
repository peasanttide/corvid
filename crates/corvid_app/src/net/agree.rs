//! Getting the peers to agree on who is where: the proposal and the roster.
//!
//! The seam against `play.rs` is that none of this is per tick. A peer joins,
//! a peer leaves, and one machine proposes the seating -- each of them once per
//! session and each of them out of band of the simulation.

use corvid_behavior::{PlayerId, State};
use corvid_net::PeerId;
use corvid_time::Tick;

use crate::net::{Control, Link, TickTraffic, halted, seat_of};

impl<S: State> Link<S> {
    /// Says what this machine thinks about who has left, folds in what everyone
    /// else thinks, and catches up whoever has just arrived.
    ///
    /// Its own method rather than the top of [`collect`](Self::collect) because
    /// it is the half that is about *machines* rather than about the session:
    /// nothing here reads a datagram, and everything here is a message about
    /// who is still in the room.
    ///
    /// # Errors
    ///
    /// Whatever applying an agreed departure reports.
    pub(super) fn agree(
        &mut self,
        gone: &[PeerId],
        heard: &[(PeerId, Control)],
        arrived: &[PeerId],
        traffic: &mut TickTraffic,
    ) -> Result<(), crate::Error> {
        let lead = u64::from(self.peer.budget.delay) + u64::from(self.peer.budget.ahead) + 1;
        let me = self.peer.seat();

        // What this machine noticed itself. The tick is far enough ahead that
        // nobody has confirmed past it -- a peer runs at most `delay + ahead`
        // beyond what every seat has spoken for -- so an agreement reached on it
        // costs no rollback.
        for peer in gone {
            let seat = seat_of(*peer);
            let at = self.peer.tick().saturating_add(lead);
            let mine = *self.mine.entry(seat).or_insert(at);
            self.say_all(Control::Departed {
                seat: seat.0,
                from: me.0,
                at: mine,
            });
            self.propose(seat, me, mine, traffic)?;
        }

        for (peer, control) in heard {
            let (seat, from, at) = match *control {
                Control::Departed { seat, from, at } => (seat, from, at),
                // Somebody cannot catch up from actions. What it needs is a
                // state, and this machine has one.
                Control::Stuck { seat, agreed } => {
                    self.send_state(*peer, PlayerId(seat), agreed)?;
                    continue;
                }
            };
            let seat = PlayerId(seat);
            // Somebody else thinks a seat has gone and this machine has not
            // noticed yet. It says what it thinks as well, because a set stays
            // incomplete until it does -- and it is never earlier than what it
            // has already simulated, which is what keeps the agreement free of
            // a rollback it did not need.
            if !self.mine.contains_key(&seat) && self.departures.is_live(seat) {
                let ours = self.peer.tick().saturating_add(lead).max(at);
                self.mine.insert(seat, ours);
                self.say_all(Control::Departed {
                    seat: seat.0,
                    from: me.0,
                    at: ours,
                });
                self.propose(seat, me, ours, traffic)?;
            }
            self.propose(seat, PlayerId(from), at, traffic)?;
        }

        // And a machine that has just become reachable is told what this one
        // has said, because a proposal it never heard is a set that never
        // completes.
        for peer in arrived {
            self.tell(*peer, me);
        }
        Ok(())
    }

    /// Folds one machine's opinion in, and applies the departure if that
    /// completed the set.
    ///
    /// **Nothing reaches the session until every seat still here has said
    /// something**, which is what [`Departures`] is for: the tick applied is the
    /// minimum over a complete set of opinions, so it is the same tick on every
    /// machine and no machine ever simulates a roster the others do not have.
    ///
    /// # Errors
    ///
    /// Whatever the rollback to the agreed tick reports.
    pub(super) fn propose(
        &mut self,
        seat: PlayerId,
        from: PlayerId,
        at: Tick,
        traffic: &mut TickTraffic,
    ) -> Result<(), crate::Error> {
        let Some(agreed) = self.departures.propose(seat, from, at) else {
            return Ok(());
        };

        let rolled = self.peer.depart(seat, agreed).map_err(halted)?;
        if rolled.ticks > traffic.rolled.ticks {
            traffic.rolled = rolled;
        }
        tracing::info!(
            name: "corvid_app.departed",
            seat = seat.0,
            at = %agreed,
            rewound = rolled.ticks,
            "every machine still here agreed this seat left, so it has",
        );
        Ok(())
    }

    /// Whether the rows this peer is waiting for are gone for good.
    ///
    /// The newest head anybody has announced, against the tick every seat has
    /// been confirmed to. A datagram's window reaches back
    /// [`CATCHUP`](corvid_lockstep::CATCHUP) rows from the sender's head, so a
    /// frontier further behind than that is a frontier no retransmission will
    /// ever reach -- the rows are not late, they are unsent.
    ///
    /// **A machine on its own is never stuck.** Nothing has announced anything,
    /// so there is nothing this is behind: an outage that stops every peer
    /// stops every peer's head too, and when it ends the windows still cover
    /// the gap. What produces a real one is a session that moved on without
    /// this machine in it.
    pub(super) fn is_stuck(&self) -> bool {
        let agreed = self.peer.frontier.agreed();
        let window = u64::try_from(corvid_lockstep::CATCHUP).unwrap_or(u64::MAX);
        self.heard_head.0 > agreed.0.saturating_add(window)
    }

    /// The seat that answers when somebody cannot catch up.
    ///
    /// The lowest one still in the session, and the choice is arbitrary in
    /// every way but one: **it has to be the same answer on every machine**.
    ///
    /// Two peers cut off from each other are both stuck and both ask, and
    /// neither is ahead of the other -- so a rule like "whoever has got further
    /// answers" leaves nobody answering, and "anybody who can, answers" has them
    /// swapping states and reopening on two different ones. One designated seat
    /// is what makes the outcome the same session on both sides, and the roster
    /// is a thing both machines already agree about.
    pub(super) fn authority(&self) -> PlayerId {
        (0..self.peer.session.log.players())
            .map(PlayerId)
            .find(|seat| self.peer.departed(*seat).is_none())
            .unwrap_or_else(|| self.peer.seat())
    }
}
