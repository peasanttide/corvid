//! Seats this machine plays besides its own, and handing one over to a
//! machine that joins a session already being played.
//!
//! The seam against `rescue.rs` is who the state is for. That file catches up
//! a machine that fell behind; this one seats a machine that was never here.
//! A host plays the seats nobody took with the game's bot, submitting for
//! each and sending each its own datagram. A machine that joins one of them
//! opens the session's opening, folds nothing in and asks for a state; the
//! host's answer names the tick the seat changes hands at -- the first the
//! host has not spoken for -- and from that tick the host says nothing more
//! for the seat and the joiner says everything. Nobody speaks twice for a
//! tick and no tick goes unspoken for, so the hand-over costs no rollback.

use std::vec::Vec;

use corvid_behavior::{PlayerId, State};
use corvid_net::PeerId;
use corvid_time::Tick;

use crate::net::{Control, Link};

/// How many ticks a joiner waits between asking for a state again.
const ASK_EVERY: u32 = 30;

impl<S: State> Link<S> {
    /// The same link, playing `bots` besides its own seat.
    pub(crate) fn with_bots(mut self, bots: Vec<PlayerId>) -> Self {
        self.peer.extra.clone_from(&bots);
        self.bots = bots;
        self
    }

    /// The same link, for a machine joining a session already being played:
    /// it plays nothing until a state arrives.
    pub(crate) fn joining(mut self) -> Self {
        self.waiting = Some(0);
        self
    }

    /// The seats this link submits for besides its own.
    pub(crate) fn bots(&self) -> &[PlayerId] {
        &self.bots
    }

    /// Seats a machine the lobby let in: which seat `peer` plays.
    pub(crate) fn admit(&mut self, peer: PeerId, seat: PlayerId) {
        self.seats.get_or_insert_default().insert(peer, seat);
    }

    /// Whether this machine is still waiting for the state it joins on.
    pub(super) const fn is_waiting(&self) -> bool {
        self.waiting.is_some()
    }

    /// Whether this machine's own seat is its to speak for at `at`: always,
    /// but on a machine that joined, from the tick it was handed.
    pub(super) fn speaks_at(&self, at: Tick) -> bool {
        self.waiting.is_none() && self.starts.is_none_or(|starts| at >= starts)
    }

    /// Asks for the state to join on, now and every [`ASK_EVERY`] ticks
    /// after, until one arrives.
    pub(super) fn ask_to_join(&mut self) {
        let Some(asked) = self.waiting else {
            return;
        };
        if asked.is_multiple_of(ASK_EVERY) {
            let seat = self.peer.seat().0;
            let agreed = self.peer.frontier.agreed();
            self.say_all(Control::Stuck { seat, agreed });
        }
        self.waiting = Some(asked.wrapping_add(1));
    }

    /// Submits the bots' actions, for the seats this link still plays.
    pub(super) fn submit_bots(
        &mut self,
        bots: Vec<(PlayerId, S::Action)>,
    ) -> Result<(), crate::Error> {
        for (seat, action) in bots {
            if self.bots.contains(&seat) {
                self.peer
                    .submit_for(seat, action)
                    .map_err(crate::net::refused)?;
            }
        }
        Ok(())
    }

    /// Every seat this link sends a datagram for: its own, once it speaks;
    /// its bots'; and a seat it handed over, until everyone has its rows.
    pub(super) fn speaking(&mut self) -> Vec<PlayerId> {
        let agreed = self.peer.frontier.agreed();
        self.handing.retain(|_, handover| agreed < *handover);
        let own = self.peer.seat();
        // A joiner's window reaches back over rows the host spoke for, so it
        // says nothing until its own first row is in the log: before that its
        // window would be the host's rows as far as it has heard them, and
        // idle padding where it has not.
        if let Some(starts) = self.starts
            && self
                .peer
                .frontier
                .confirmed(own)
                .is_some_and(|had| had >= starts)
        {
            self.starts = None;
        }
        let mut seats = Vec::new();
        if self.waiting.is_none() && self.starts.is_none() {
            seats.push(own);
        }
        seats.extend(self.bots.iter().copied());
        seats.extend(self.handing.keys().copied());
        seats
    }

    /// Hands `seat` over, if this link is playing it: the tick the joiner
    /// speaks from. The bot stops, and the joiner's acknowledgements count.
    pub(super) fn hand_over(&mut self, seat: PlayerId) -> Option<Tick> {
        if let Some(handover) = self.handing.get(&seat) {
            return Some(*handover);
        }
        if !self.bots.contains(&seat) {
            return None;
        }
        let handover = self
            .peer
            .frontier
            .confirmed(seat)
            .map_or_else(|| self.peer.tick().next(), Tick::next);
        self.bots.retain(|bot| *bot != seat);
        self.peer.extra.retain(|bot| *bot != seat);
        self.handing.insert(seat, handover);
        tracing::info!(
            name: "corvid_app.handing_over",
            seat = seat.0,
            at = %handover,
            "a machine joined; the seat's bot stops here and the machine plays on",
        );
        Some(handover)
    }
}
