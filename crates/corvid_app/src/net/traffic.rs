//! What a linked run reports about itself, and the control messages it sends.
//!
//! The seam against `mod.rs` is that nothing here holds a peer. These are the
//! counts a caller reads out of an [`Outcome`](crate::Outcome) and the
//! messages that cross the control channel, and both are data.

use std::collections::BTreeMap;

use corvid_behavior::{PlayerId, State};
use corvid_lockstep::{Advanced, Rolled};
use corvid_time::Tick;

/// What **one tick** over a transport did, for whatever draws an overlay of it.
///
/// Copy and small, because a runtime keeps the newest one and a lab keeps a
/// window of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TickTraffic {
    /// What [`Peer::advance`](corvid_lockstep::Peer::advance) did.
    pub advanced: Advanced,
    /// The deepest rollback this tick's arrivals caused, and a zeroed
    /// [`Rolled`] where none did.
    pub rolled: Rolled,
    /// How many datagrams were folded in this tick.
    pub heard: u16,
    /// How many were sent.
    pub sent: u16,
    /// How many arrived and could not be decoded, which is a foreign packet or
    /// a corrupted one and is never a reason to stop.
    pub undecodable: u16,
    /// How many peers the transport was reaching when the sending happened.
    pub peers: u16,
    /// Whether a whole state arrived this tick and was adopted, because no
    /// window of actions could have caught this machine up.
    pub rescued: bool,
}

/// What a **whole run** over a transport did, which is what
/// [`Outcome::traffic`](crate::Outcome::traffic) carries.
///
/// Counted rather than sampled: a run that rolled back four hundred times and a
/// run that rolled back once look identical in the newest [`TickTraffic`],
/// and which of the two happened is the thing anybody asking wants to know.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Traffic {
    /// How many datagrams were folded in.
    pub heard: u64,
    /// How many were sent.
    pub sent: u64,
    /// How many arrived and could not be read.
    pub undecodable: u64,
    /// How many corrections rolled this peer back.
    pub rollbacks: u64,
    /// How many ticks were re-simulated over all of them.
    pub resimulated: u64,
    /// The deepest single rollback, in ticks.
    pub deepest: u8,
    /// How many times this machine was handed a whole state because no window
    /// of actions could catch it up.
    pub rescues: u64,
    /// How many ticks this peer declined to simulate because it was
    /// [`Budget::ahead`](corvid_lockstep::Budget) past the tick every seat had
    /// confirmed.
    ///
    /// Stalling is a decision rather than a failure -- a visible hitch is better
    /// than predicting a decision nobody has made -- and this is how often it
    /// was made.
    pub stalls: u64,
}

impl Traffic {
    /// Folds one tick's traffic into the totals.
    pub(super) fn fold(&mut self, tick: TickTraffic) {
        self.heard = self.heard.saturating_add(u64::from(tick.heard));
        self.sent = self.sent.saturating_add(u64::from(tick.sent));
        self.undecodable = self.undecodable.saturating_add(u64::from(tick.undecodable));
        if tick.rolled.happened() {
            self.rollbacks = self.rollbacks.saturating_add(1);
            self.resimulated = self
                .resimulated
                .saturating_add(u64::from(tick.rolled.ticks));
            self.deepest = self.deepest.max(tick.rolled.ticks);
        }
        if tick.advanced.stalled {
            self.stalls = self.stalls.saturating_add(1);
        }
        if tick.rescued {
            self.rescues = self.rescues.saturating_add(1);
        }
    }
}

/// What this runtime says to another one over
/// [`Channel::Control`](corvid_net::Channel).
///
/// One variant so far. It is an enum rather than a bare struct because the
/// channel is a stream of *messages* and a reader that assumed which one it was
/// holding would be a reader that could not be added to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(super) enum Control {
    /// A seat left, on this tick.
    ///
    /// Sent by whichever machine noticed, to everyone still reachable, and
    /// folded in by [`Peer::depart`](corvid_lockstep::Peer::depart) -- which
    /// keeps the earliest and refuses to move one later, so two machines that
    /// noticed at different moments agree without a round trip.
    /// This machine cannot catch up from actions and is asking for a state.
    ///
    /// Sent by a peer that has been stalled longer than any window of history
    /// can reach back, which is what a link that went away for a while leaves
    /// behind. What answers it is a [`Transfer`](Channel::Transfer).
    Stuck {
        /// Which seat is asking.
        seat: u16,
        /// The newest tick it has every seat's action for, so the answer can be
        /// a state it will accept.
        agreed: Tick,
    },
    Departed {
        /// Which seat left.
        seat: u16,
        /// Which seat is saying so. Carried rather than taken from the
        /// transport, because the agreement is over *seats* and the map from a
        /// connection to a seat is this runtime's rather than the wire's.
        from: u16,
        /// The tick the sender thinks it stopped being in the session.
        at: Tick,
    },
}

/// A whole state, on its way to a machine that cannot catch up without one.
///
/// The roster's departures ride with it, because a state alone is not a
/// session: a machine that adopted the state and went on simulating a seat
/// everybody else had agreed was gone would diverge on its first tick.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound = "")]
pub(super) struct Transfer<S: State> {
    /// Which tick the state is at.
    pub(super) at: Tick,
    /// The state itself.
    pub(super) state: S,
    /// Every seat that has left, and when.
    pub(super) departed: Vec<(u16, Tick)>,
}

/// Who has proposed what about which seat leaving, and what has been agreed.
///
/// # Why a departure needs agreeing at all
///
/// [`Peer::depart`](corvid_lockstep::Peer::depart) changes what a machine
/// simulates from the tick it names, so two machines that applied different
/// ticks would compute different states -- and, because every peer sends a
/// digest of its state every tick, they would report each other as *desynced*
/// while they were merely uninformed. Keeping the earliest proposal fixes the
/// end state and not the middle: the machine that guessed later would have
/// played, sent and been judged on ticks nobody else agreed with.
///
/// So nothing is applied until everybody still here has said what they think.
/// Each machine proposes a tick, hears the others', and applies the **minimum
/// over a complete set** -- which is the same number on every machine, because
/// it is the same set. Until the set is complete nothing is applied and nothing
/// diverges; the session is already stalled against the seat that stopped
/// speaking, so the wait costs what it was costing anyway.
///
/// # What it does not do
///
/// It is not a general consensus. There is no leader, no term and no proof
/// against a machine that lies -- a peer that proposed one tick and simulated
/// another would desync, and would be reported. What it is is enough for the
/// one decision a runtime has to make without a server: everybody agreed the
/// same number, or nobody acted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Departures {
    /// How many seats the session has.
    seats: u16,
    /// Proposals, by the seat leaving and then by the seat proposing.
    proposed: BTreeMap<PlayerId, BTreeMap<PlayerId, Tick>>,
    /// What has been agreed and applied.
    agreed: BTreeMap<PlayerId, Tick>,
}

impl Departures {
    /// A table for a session of `seats` seats, with nobody gone.
    #[must_use]
    pub const fn new(seats: u16) -> Self {
        Self {
            seats,
            proposed: BTreeMap::new(),
            agreed: BTreeMap::new(),
        }
    }

    /// Records one machine's opinion about when a seat left.
    ///
    /// Answers the agreed tick on the proposal that completes the set, and
    /// [`None`] every other time -- including for a seat already agreed, because
    /// a departure is applied once.
    ///
    /// A proposal from a seat that has already left is recorded and does not
    /// count towards the set, which is what stops a session waiting for an
    /// opinion from somebody who has gone.
    pub fn propose(&mut self, seat: PlayerId, from: PlayerId, at: Tick) -> Option<Tick> {
        if self.agreed.contains_key(&seat) {
            return None;
        }
        let opinions = self.proposed.entry(seat).or_default();
        // The earliest a machine ever said, so that hearing the same machine
        // twice cannot move the answer later.
        opinions
            .entry(from)
            .and_modify(|had| {
                if at < *had {
                    *had = at;
                }
            })
            .or_insert(at);

        let missing = (0..self.seats)
            .map(PlayerId)
            .filter(|live| *live != seat)
            .filter(|live| !self.agreed.contains_key(live))
            .any(|live| {
                !self
                    .proposed
                    .get(&seat)
                    .is_some_and(|by| by.contains_key(&live))
            });
        if missing {
            return None;
        }

        let agreed = self.proposed.get(&seat)?.values().copied().min()?;
        self.agreed.insert(seat, agreed);
        Some(agreed)
    }

    /// Takes a departure decided elsewhere as a fact.
    ///
    /// A machine being rescued with a state was not part of the set that agreed
    /// this, and is in no position to argue: the session it is adopting has the
    /// seat gone from that tick, so this is what "the same session" means. It
    /// is the one way in with no quorum behind it, and it exists because the
    /// alternative is a rescued machine simulating a roster nobody else has.
    pub fn adopt(&mut self, seat: PlayerId, at: Tick) {
        let earliest = self
            .agreed
            .get(&seat)
            .map_or(at, |already| if *already < at { *already } else { at });
        self.agreed.insert(seat, earliest);
    }

    /// The tick a seat left on, once it has been agreed.
    #[must_use]
    pub fn agreed(&self, seat: PlayerId) -> Option<Tick> {
        self.agreed.get(&seat).copied()
    }

    /// Every departure that has been agreed, as a seat and a tick.
    pub fn all(&self) -> impl Iterator<Item = (PlayerId, Tick)> + '_ {
        self.agreed.iter().map(|(seat, at)| (*seat, *at))
    }

    /// Whether a seat is still in the session.
    #[must_use]
    pub fn is_live(&self, seat: PlayerId) -> bool {
        !self.agreed.contains_key(&seat)
    }
}
