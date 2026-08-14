//! When actions are not enough: a whole state, sent and adopted.
//!
//! The seam against `agree.rs` is what crosses the wire. That file moves
//! *seats*; this one moves a *state*, which is what a peer that has fallen
//! outside the rollback window needs and the one thing a datagram cannot
//! carry.

use corvid_behavior::{PlayerId, State};
use corvid_net::{Channel, PeerId};
use corvid_time::Tick;

use crate::net::{Link, TickTraffic, Transfer, halted, seat_of};

impl<S: State> Link<S> {
    /// Answers a peer that says it cannot catch up, with a state.
    ///
    /// Sent over [`Channel::Transfer`], which is reliable and ordered and is
    /// the one channel sized for something this big -- an action datagram is a
    /// handful of bytes and this is a whole `State`.
    ///
    /// Only [`authority`](Self::authority) answers, and it answers whether or
    /// not it is ahead: what a stuck machine needs is not a *better* state but
    /// the *same* state as everybody else, and one seat deciding that is what
    /// makes it the same on every machine.
    ///
    /// **The sender reopens on its own state as well.** Otherwise it goes on
    /// waiting for rows the rescued machine will never send -- they are older
    /// than the tick it just restarted at -- and the session ends with one peer
    /// playing and one peer stuck, which is the failure it was fixing wearing
    /// the other hat.
    ///
    /// # Errors
    ///
    /// Whatever reopening this machine's own session reports.
    pub(super) fn send_state(
        &mut self,
        to: PeerId,
        seat: PlayerId,
        agreed: Tick,
    ) -> Result<(), crate::Error> {
        if self.authority() != self.peer.seat() {
            tracing::debug!(
                name: "corvid_app.not_the_authority",
                peer = %to,
                seat = seat.0,
                asked_from = %agreed,
                "a peer asked for a state and this machine is not the one that answers",
            );
            return Ok(());
        }
        let transfer = Transfer::<S> {
            at: self.peer.tick(),
            state: S::clone(self.peer.state()),
            departed: self
                .departures
                .all()
                .map(|(seat, at)| (seat.0, at))
                .collect(),
        };
        let Ok(bytes) = corvid_wire::encode(&transfer) else {
            tracing::error!(
                name: "corvid_app.unencodable_transfer",
                "this machine's state could not be encoded, so nobody can be rescued with it",
            );
            return Ok(());
        };
        tracing::info!(
            name: "corvid_app.sending_state",
            peer = %to,
            at = %transfer.at,
            bytes = bytes.len(),
            "answering a peer that cannot catch up from actions",
        );
        if let Err(why) = self.transport.send_stream(to, Channel::Transfer, &bytes) {
            tracing::warn!(
                name: "corvid_app.unsent_transfer",
                peer = %to,
                why = %why,
                "a state transfer did not go",
            );
            return Ok(());
        }

        // And this machine restarts there too, so that it stops waiting for the
        // rows the peer it just rescued is never going to send.
        let at = transfer.at;
        let state = S::clone(self.peer.state());
        self.peer.resync(at, state).map_err(halted)?;
        Ok(())
    }

    /// The state to adopt out of everything that arrived, if any of it counts.
    ///
    /// **Only from the authority.** Adopting a state assigns this machine's
    /// tick and its whole simulation outright and forgets every row before
    /// them, so which peer sent one decides what this machine is playing.
    /// [`send_state`](Self::send_state) already refuses to answer unless this
    /// machine is the authority; this is that same rule read from the receiving
    /// end, which is the end it was missing from -- any peer that cared to send
    /// a `Transfer` was obeyed.
    ///
    /// The newest wins among what is left, because the authority may have
    /// answered twice -- a second `Stuck` sent while the first answer was still
    /// in flight is one request, and the later state is the more useful one.
    ///
    /// # What this does not check
    ///
    /// That this machine asked. [`is_stuck`](Self::is_stuck) is the condition
    /// under which it sends a `Stuck`, and a flag set there and cleared here
    /// would refuse a state the authority pushed unprompted. That is worth
    /// having against an authority that is merely *wrong* rather than hostile,
    /// and it is deliberately not here yet: it changes when a legitimate rescue
    /// is accepted, and the arrival window for one is exactly the case this
    /// crate's tests do not yet drive.
    pub(super) fn solicited(&self, transferred: Vec<(PeerId, Transfer<S>)>) -> Option<Transfer<S>> {
        let authority = self.authority();
        transferred
            .into_iter()
            .filter(|(from, _)| {
                if seat_of(*from) == authority {
                    return true;
                }
                tracing::warn!(
                    name: "corvid_app.unsolicited_transfer",
                    peer = %from,
                    authority = authority.0,
                    "a state arrived from a seat that does not answer for this \
                     session; dropped",
                );
                false
            })
            .max_by_key(|(_, transfer)| transfer.at)
            .map(|(_, transfer)| transfer)
    }

    /// Adopts a state somebody sent, departures and all.
    ///
    /// # Who may call this
    ///
    /// [`collect`](Self::collect) decides, through
    /// [`solicited`](Self::solicited), and it is the only caller: the state has
    /// to have come from [`authority`](Self::authority). Adopting one assigns
    /// the tick and the state outright and forgets every row before them, so
    /// which peer sent it is what decides what this machine plays.
    ///
    /// # What this still trusts
    ///
    /// The authority. A designated seat that lies about `at` or about the state
    /// is a seat that decides where this session goes, and nothing here checks
    /// its answer against one this machine derived -- there is nothing to check
    /// it against, which is the whole reason a rescue exists. So the roster is
    /// the trust boundary: peers are other players, not arbitrary senders. A
    /// deployment where they are not wants authentication under
    /// [`Transport`](corvid_net::Transport), not a further test here.
    ///
    /// # Errors
    ///
    /// [`Error::Halted`](crate::Error::Halted) for a state at a tick this
    /// session cannot reach, which is one before its opening.
    pub(super) fn rescue(
        &mut self,
        transfer: Transfer<S>,
        traffic: &mut TickTraffic,
    ) -> Result<(), crate::Error> {
        // The roster first. A machine that adopted the state and went on
        // simulating a seat everybody else had agreed was gone would diverge on
        // its very first tick -- so the departures are applied before the state
        // rather than after it.
        for (seat, at) in transfer.departed {
            let seat = PlayerId(seat);
            if self.departures.agreed(seat).is_none() {
                // Agreed elsewhere, and arriving here as a fact rather than as
                // an opinion: a machine being rescued was not part of the set
                // that decided it and is in no position to argue.
                self.departures.adopt(seat, at);
                self.mine.insert(seat, at);
            }
            let _rolled = self.peer.depart(seat, at).map_err(halted)?;
        }

        self.peer
            .resync(transfer.at, transfer.state)
            .map_err(halted)?;
        traffic.rescued = true;
        tracing::info!(
            name: "corvid_app.rescued",
            at = %transfer.at,
            "this machine adopted a state, because no window of actions could reach it",
        );
        Ok(())
    }
}
