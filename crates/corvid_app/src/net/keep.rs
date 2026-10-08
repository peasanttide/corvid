//! Keeping a seat whose machine went, and writing down a save.
//!
//! The seam against `join.rs` is direction. That file hands a seat the bot
//! plays to a machine that arrived; this one hands the seat of a machine that
//! went back to the bot, so it can be joined again rather than leaving the
//! session for good. And a save: what a final tick asked to keep, read back
//! from the states this peer already computed.

use std::sync::Arc;

use corvid_behavior::{PlayerId, SaveSlot, State};
use corvid_replay::{Opening, Session};

use crate::net::Link;

impl<S: State> Link<S> {
    /// The same link, keeping the seat of a machine that goes for its bot to
    /// play: a lobby's host's.
    pub(crate) const fn keeping(mut self) -> Self {
        self.keeps = true;
        self
    }

    /// Whether a machine that goes leaves its seat to this link's bot.
    pub(super) const fn keeps(&self) -> bool {
        self.keeps
    }

    /// Plays `seat` with the bot from here on, the rows its machine never
    /// sent confirmed as this machine predicted them
    /// ([`Peer::take_over`](corvid_lockstep::Peer::take_over)), until a
    /// machine joins it again. Nothing for a link that does not keep seats,
    /// for its own seat, or for one it already plays.
    pub(crate) fn keep(&mut self, seat: PlayerId) {
        if !self.keeps || seat == self.peer.seat() || self.bots.contains(&seat) {
            return;
        }
        // A seat handed to a machine that has gone again is the bot's again.
        self.handing.remove(&seat);
        match self.peer.take_over(seat) {
            Ok(from) => {
                self.bots.push(seat);
                tracing::info!(
                    name: "corvid_app.seat_kept",
                    seat = seat.0,
                    from = %from,
                    "a machine went; the bot plays its seat until somebody joins it",
                );
            }
            Err(why) => tracing::warn!(
                name: "corvid_app.seat_unkept",
                seat = seat.0,
                %why,
                "a seat whose machine went could not be kept for the bot",
            ),
        }
    }

    /// Moves the saves the peer's final ticks asked for into this link's
    /// list, for [`saved`](Self::saved) to answer.
    pub(super) fn note_saves(&mut self) {
        self.saving.extend(self.peer.take_saved());
    }

    /// What a save into `slot` holds: a session opening on the state the
    /// tick that asked for it produced, and that state -- the same tick and
    /// the same state on every machine, because the tick is final. The
    /// session holds no actions, so a load opens on the state at once rather
    /// than replaying anything. [`None`] if no final tick asked for `slot`,
    /// or the state can no longer be reached.
    pub(crate) fn saved(&mut self, slot: SaveSlot) -> Option<(Session<S>, S)> {
        let index = self.saving.iter().position(|(_, asked)| *asked == slot)?;
        let (asked, _) = self.saving.remove(index);
        let at = asked.next();
        let state = self.peer.final_state(at)?;
        let session = &self.peer.session;
        let opening = Opening {
            level: session.opening.level.clone(),
            content: Arc::clone(session.levels.at(at)),
            rules: Arc::clone(&session.opening.rules),
            roster: session.opening.roster.clone(),
            seed: session.opening.seed,
            first: at,
            origin: Some(Arc::new(S::clone(&state))),
            schema: session.opening.schema,
        };
        let session = Session::new(opening).ok()?;
        Some((session, state))
    }

    /// Whether a datagram for `seat` is one to fold in: not for a seat this
    /// link's bot speaks for, which a machine that went may still have rows
    /// in flight for. (Its own seat's rows it does fold in: a machine that
    /// joined is sent the host's rows for its seat up to the hand-over.)
    pub(super) fn hears_for(&self, seat: PlayerId) -> bool {
        !self.bots.contains(&seat)
    }
}
