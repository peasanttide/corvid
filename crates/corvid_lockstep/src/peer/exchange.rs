//! What the other machines say, and what this one does about it.
//!
//! The seam against `speak.rs` is direction: everything here is inbound. A
//! datagram, a mark and a departure all arrive from somewhere else, and all
//! three can roll this peer back.

use alloc::vec::Vec;

use corvid_behavior::{PlayerId, State};
use corvid_hash::Digest;
use corvid_replay::Unreachable;
use corvid_time::Tick;

use crate::{
    Advanced, Correction, Datagram, Desync, Halt, Peer, Resync, Rolled, absorb, predict::predict,
};

impl<S: State> Peer<S> {
    /// Folds in what arrived, rolling back if it must.
    ///
    /// # Errors
    ///
    /// [`Halt::Refused`](crate::Halt::Refused) for a datagram naming a tick past
    /// [`Budget::horizon`](crate::Budget::horizon), which is the denial-of-service arm: a tick number
    /// is the one thing in a session that arrives from somewhere else, and a
    /// log that grew to whatever it said would be a request for as much memory
    /// as the number. [`Halt::Contradiction`](crate::Halt::Contradiction) for a peer that has sent two
    /// different actions for one tick, and [`Halt::Desync`](crate::Halt::Desync) for one whose mark
    /// disagrees with this peer's own.
    /// # No sink
    ///
    /// A datagram never reaches one. All this can do is roll back, and a tick
    /// replayed to work off a rollback is this machine recomputing a state
    /// rather than the game asking for anything a second time -- so there is
    /// nothing here for a sink to receive.
    pub fn receive(&mut self, datagram: &Datagram<S::Action>) -> Result<Rolled, Halt> {
        // How far this machine is willing to make room for, and never further:
        // a tick number is the one thing in a session that arrives from
        // somewhere else, and a log that grew to whatever it said would be a
        // request for as much memory as the number.
        //
        // A datagram past it is **clamped rather than refused**. A peer that
        // started a second later than its opponent receives windows whose
        // newest rows are past its horizon on every packet, and refusing them
        // would end the session before it began; the rows it can use are folded
        // in, the rest stay in the sender's window until this machine
        // acknowledges them, and it catches up. The same clamp is what stops a
        // stranger ending a game by sending one large number.
        let horizon = self.budget.horizon(self.tick);
        let head = datagram.head();
        // Entirely about ticks this session no longer holds. A bounded run
        // forgets its far past and a `resync` forgets everything before the
        // state it adopted, so a datagram that was in flight across either of
        // those arrives naming rows that are gone -- which is ordinary, and is
        // not something to stop for.
        if head < self.session.first() {
            return Ok(Rolled::default());
        }
        let ceiling = if head < horizon { head } else { horizon };
        self.session.log.extend_to(ceiling)?;

        // What the sender says it has, which is what decides how far back this
        // peer's own window reaches from now on. Never backwards: a reordered
        // datagram carries an older acknowledgement and un-acknowledging a row
        // would put it back in every packet.
        if let Some(heard) = self.heard.get_mut(usize::from(datagram.seat.0))
            && *heard < datagram.heard
        {
            *heard = datagram.heard;
        }

        let correction = absorb(&mut self.session.log, &mut self.frontier, datagram)?;
        let rolled = match correction {
            Correction::Agreed | Correction::Duplicate => Rolled::default(),
            Correction::Contradiction { at } => {
                return Err(Halt::Contradiction {
                    peer: datagram.seat,
                    at,
                });
            }
            Correction::Mispredicted { at } => self.roll_back(at)?,
        };

        self.check_mark(datagram.seat, datagram.marked, datagram.mark)?;
        Ok(rolled)
    }

    /// Simulates one tick forward, predicting whatever has not arrived.
    ///
    /// # Errors
    ///
    /// [`Halt::Refused`](crate::Halt::Refused) if the log could not be grown to the tick being
    /// simulated.
    /// # Only a tick's first simulation reaches the sink
    ///
    /// A rollback re-simulates ticks this peer has already been through, and a
    /// [`save`](corvid_behavior::Command::save) is a file rather than a value --
    /// so a peer on a link that mispredicts every second tick would write one
    /// save per correction if the re-simulation reached `command` too. It does
    /// not: a replayed tick is handed a
    /// [`Discard`](corvid_behavior::Discard) instead.
    ///
    /// What that rule costs is worth stating rather than leaving to be found. A
    /// networked game reaches the same command stream a single-seat one does
    /// **for the ticks that were never corrected**, and that is the honest form
    /// of the claim: a command from a tick whose prediction turned out wrong
    /// was asked for by a state that never happened, and nothing here can unsay
    /// it. A game whose requests must survive that puts the request in its
    /// `State` and lets the client read it out of a confirmed tick.
    pub fn advance(
        &mut self,
        command: &mut impl corvid_behavior::Command,
    ) -> Result<Advanced, Halt> {
        let ceiling = self
            .frontier
            .agreed()
            .saturating_add(u64::from(self.budget.ahead));
        if self.tick >= ceiling {
            return Ok(Advanced {
                tick: self.tick,
                predicted_seats: 0,
                stalled: true,
            });
        }

        let predicted = predict(&mut self.session.log, &self.frontier, self.tick)?;
        self.simulate_one(command);
        if self.tick > self.resume {
            self.resume = self.tick;
        }
        Ok(Advanced {
            tick: self.tick,
            predicted_seats: predicted.seats,
            stalled: self.stalled(),
        })
    }

    /// Compares an arrived mark against this peer's own trace.
    ///
    /// A tick this peer has no mark for says nothing, which is what a peer that
    /// has not got there yet honestly knows.
    ///
    /// # Errors
    ///
    /// [`Halt::Desync`](crate::Halt::Desync) when the two digests differ, naming the tick they
    /// differ at rather than the tick the mark arrived on.
    pub fn compare(&self, seat: PlayerId, at: Tick, mark: Digest) -> Result<(), Halt> {
        let Some(local) = self.session.marks.get(at) else {
            return Ok(());
        };
        if local == mark {
            return Ok(());
        }
        Err(Halt::Desync(Desync {
            at,
            peer: seat,
            agreed_through: self.agreed_marks,
            local,
            remote: mark,
            fields: Vec::new(),
            first_divergent: None,
        }))
    }

    /// Records that a seat has left, on a tick every machine agrees on.
    ///
    /// A runtime calls this when its transport reports that a peer has gone.
    /// Without it, a player who closes their window leaves everybody else
    /// stalled against a frontier that will never move again -- the session does
    /// not desync, it simply stops, which is the worse of the two failures
    /// because nothing reports it.
    ///
    /// # Why this takes a tick, and why that makes it safe
    ///
    /// A departure changes what every machine simulates: from `at` the seat is
    /// [`Presence::Dropped`](corvid_behavior::Presence) and submits
    /// `Action::default()` for ever. So two machines that decided it on
    /// different ticks would compute different states -- which is a desync, and
    /// it would be this crate's fault.
    ///
    /// What makes it safe is that the tick is part of the *session* rather than
    /// a decision each machine makes for itself: it is written into
    /// [`Profile::left`](corvid_replay::Profile), it is what a save carries and
    /// a replay reproduces, and **the earliest one wins**. Two runtimes that
    /// propose different ticks for the same seat both end up applying the lower
    /// of the two, in either order, because this refuses to move a departure
    /// later. That is the whole of the agreement protocol, and it needs no
    /// round trip: a proposal is idempotent and commutative, so a runtime can
    /// broadcast what it believes and fold in what it hears.
    ///
    /// The rollback is the other half. A departure at a tick this peer has
    /// already simulated past invalidates every state after it, so this rewinds
    /// to `at` and re-simulates -- exactly as a late action does, and through the
    /// same budget.
    ///
    /// # It is one-way
    ///
    /// **Nothing un-departs a seat.** A machine that comes back is a machine
    /// with a state nobody else's session agrees with, and what makes it
    /// playable again is a whole state transferred to it and
    /// [`adopt`](Self::adopt)ed -- at which point it is holding this session's
    /// roster, departures and all. Moving a `left` tick later, or clearing it,
    /// would be this machine editing history every other machine has already
    /// simulated.
    ///
    /// # No sink
    ///
    /// Like [`receive`](Self::receive): all this can do is roll back, and a
    /// replayed tick asks the runtime for nothing.
    ///
    /// # Errors
    ///
    /// [`Halt::Unreachable`](crate::Halt::Unreachable) for a tick before the session's opening, and
    /// whatever a rollback to `at` reports.
    pub fn depart(&mut self, seat: PlayerId, at: Tick) -> Result<Rolled, Halt> {
        let first = self.session.first();
        if at < first {
            return Err(Unreachable::Before { to: at, first }.into());
        }
        let Some(profile) = self.session.opening.roster.get_mut(usize::from(seat.0)) else {
            // A seat this session does not have. Nothing to record, and nothing
            // to report either: a transport can name a peer no roster seats.
            return Ok(Rolled::default());
        };
        // The earliest wins, and a departure already at or before this one is
        // the whole answer.
        if profile.left.is_some_and(|already| already <= at) {
            return Ok(Rolled::default());
        }
        profile.left = Some(at);
        self.frontier.retire(seat);

        // Every state from `at` on was computed with this seat present, so they
        // are all wrong now. The correction lands on `at` for the reason every
        // other correction does: the state *at* a tick is what the rows before
        // it produced, and what changed is the roster the tick at `at` is
        // simulated with.
        self.roll_back(at)
    }

    /// The tick a seat left on, if it has.
    #[must_use]
    pub fn departed(&self, seat: PlayerId) -> Option<Tick> {
        self.session
            .opening
            .roster
            .get(usize::from(seat.0))
            .and_then(|profile| profile.left)
    }

    /// Asks for a whole state, which is what a build without `dev` does instead
    /// of bisecting.
    #[must_use]
    pub const fn resync_request(&self, at: Tick) -> Resync {
        Resync {
            seat: self.seat,
            at,
            agreed_through: self.agreed_marks,
        }
    }
}
