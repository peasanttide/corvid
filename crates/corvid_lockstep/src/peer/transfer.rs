//! Being handed a state rather than actions.
//!
//! The seam against `exchange.rs` is what arrives: a datagram carries inputs
//! and is replayed, and a transfer carries a whole state and replaces one.
//! That is the path a peer takes when it can no longer catch up from actions
//! at all.

use corvid_behavior::{PlayerId, State};
use corvid_hash::digest;
use corvid_replay::Unreachable;
use corvid_time::Tick;

use crate::{Frontier, Halt, Peer};

impl<S: State> Peer<S> {
    /// Adopts a state that arrived over a reliable channel.
    ///
    /// The snapshot ring is emptied rather than corrected: two peers' states
    /// share no history this machine can compare, so nothing it is holding can
    /// be trusted to be about the session that is resuming.
    ///
    /// Nothing here simulates, so no scratch passes through.
    ///
    /// A state for a tick this peer has already reached replaces what it
    /// computed there, and the trace behind it is kept -- that is a machine
    /// being told it was wrong about a stretch it played. For a state from
    /// *ahead* of this peer, which is what rescues one that can no longer catch
    /// up from actions, see [`resync`](Self::resync).
    ///
    /// # Errors
    ///
    /// [`Halt::Unreachable`](crate::Halt::Unreachable) for a tick before the session's opening or after
    /// the one this peer has reached.
    pub fn adopt(&mut self, at: Tick, state: S) -> Result<(), Halt> {
        let first = self.session.first();
        if at < first {
            return Err(Unreachable::Before { to: at, first }.into());
        }

        if at > self.tick {
            return Err(Unreachable::After {
                to: at,
                last: self.tick,
            }
            .into());
        }

        self.snapshots.clear();
        self.state = state;
        self.tick = at;
        self.resume = at;
        // The ticks after `at` are about to be simulated from a state that
        // arrived from another machine, so none of them has been simulated
        // *from this state* before and every one of them is a first time. A
        // high-water mark left where it was would silence the commands of every
        // tick between here and where this peer had got to.
        self.reached = at;
        self.depth = 0;
        self.session.marks.truncate_from(at);
        self.session.marks.push(digest(&self.state));
        self.agreed_marks = at;
        self.snapshots.keep(&self.session.log, at, &self.state);
        Ok(())
    }

    /// Reopens the session at `at` on `state`, forgetting everything before it.
    ///
    /// **This is what ends a stall no window of actions can end.** A peer whose
    /// link was down for longer than [`CATCHUP`](crate::CATCHUP) rows is
    /// missing actions nobody still sends: it is not behind, it is *stuck*, and
    /// no amount of waiting fixes it. What does is somebody's whole state, and
    /// what this does with one is refuse to pretend about the gap -- the rows
    /// and marks before `at` are dropped, exactly as a bounded run drops its
    /// far past, and the session begins again there.
    ///
    /// The frontier is rebuilt with it, which is the half that is easy to miss:
    /// a peer that kept waiting for the rows it was waiting for before would
    /// adopt a state and stall again immediately, for the same reason.
    ///
    /// # Both ends do this
    ///
    /// The machine that *sends* a state calls this too, with its own. Otherwise
    /// it goes on waiting for rows the rescued machine will never send -- they
    /// are older than the tick it just restarted at -- and the session ends with
    /// one peer playing and one peer stuck, which is the failure it was trying
    /// to fix wearing the other hat.
    ///
    /// # Errors
    ///
    /// [`Halt::Unreachable`](crate::Halt::Unreachable) for a tick before the session's opening, and
    /// [`Halt::Refused`](crate::Halt::Refused) if the log could not be grown to reach it.
    pub fn resync(&mut self, at: Tick, state: S) -> Result<(), Halt> {
        let first = self.session.first();
        if at < first {
            return Err(Unreachable::Before { to: at, first }.into());
        }

        self.session.log.extend_to(at)?;
        let origin = alloc::sync::Arc::new(S::clone(&state));
        // The log was grown to `at` a line ago and `at` is at or after the
        // opening, so neither refusal this can answer is reachable -- and it is
        // reported rather than ignored, so that it stays unreachable if either
        // of those changes.
        if self.session.forget_before(at, origin).is_err() {
            return Err(Unreachable::After {
                to: at,
                last: self.session.last(),
            }
            .into());
        }

        self.frontier = Frontier::new(self.session.log.players());
        for (seat, profile) in self.session.opening.roster.iter().enumerate() {
            if profile.left.is_some()
                && let Ok(seat) = u16::try_from(seat)
            {
                self.frontier.retire(PlayerId(seat));
            }
        }
        self.heard.iter_mut().for_each(|heard| *heard = None);

        self.snapshots.clear();
        self.state = state;
        self.tick = at;
        self.resume = at;
        self.reached = at;
        self.depth = 0;
        self.session.marks.truncate_from(at);
        self.session.marks.push(digest(&self.state));
        self.agreed_marks = at;
        self.snapshots.keep(&self.session.log, at, &self.state);
        Ok(())
    }

    /// The newest state this peer holds at or before `at`, and the tick it is
    /// the state at.
    ///
    /// The opening when the ring holds nothing usable, because the opening is
    /// always somewhere to start from.
    ///
    /// # Errors
    ///
    /// [`Halt::Unreachable`](crate::Halt::Unreachable) for a tick before the session's opening.
    pub fn restore(&self, at: Tick) -> Result<(Tick, S), Halt> {
        let first = self.session.first();
        if at < first {
            return Err(Unreachable::Before { to: at, first }.into());
        }
        // The opening's origin is resolved into a handle before the borrow,
        // because `origin()` answers an owned `Arc` -- a fresh session's is
        // built from `S::default()` rather than held anywhere -- and a
        // reference into it would not outlive the expression.
        Ok(match self.snapshots.nearest(&self.session.log, at) {
            Some((tick, state)) => (tick, state.clone()),
            None => (first, S::clone(&self.session.opening.origin())),
        })
    }
}
