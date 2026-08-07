//! Playing over a transport: the peer, the wire, and the tick that joins them.
//!
//! Everything here is behind the `net` feature, and everything a game sees of
//! it is one builder call. [`App::transport`](crate::App::transport) hands over
//! a `Box<dyn Transport>`; from there the loop owns a
//! [`Peer`](corvid_lockstep::Peer) and this module is what it drives per tick.
//! `State` and `Present` are untouched — a game that plays over a network
//! and a game that does not are the same two implementations, which is the
//! claim the whole lockstep design exists to support.

use corvid_replay::LevelRef;
use std::{collections::BTreeMap, vec::Vec};

use corvid_behavior::{PlayerId, State};
use corvid_lockstep::{Advanced, Budget, Datagram, Halt, Peer, Rolled};
use corvid_net::{Channel, Delivery, PeerId, SendError, Transport};
use corvid_replay::Refused;
use corvid_replay::Session;
use corvid_time::Tick;

/// What one tick over a transport did, for whatever draws an overlay of it.
///
/// Copy and small, because a runtime keeps the newest one and a lab keeps a
/// window of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Traffic {
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

/// What a whole run over a transport did, which is what
/// [`Outcome::traffic`](crate::Outcome) carries.
///
/// Counted rather than sampled: a run that rolled back four hundred times and a
/// run that rolled back once look identical in the newest tick's [`Traffic`],
/// and which of the two happened is the thing anybody asking wants to know.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Played {
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
    /// Stalling is a decision rather than a failure — a visible hitch is better
    /// than predicting a decision nobody has made — and this is how often it
    /// was made.
    pub stalls: u64,
}

impl Played {
    /// Folds one tick's traffic into the totals.
    fn fold(&mut self, tick: Traffic) {
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
enum Control {
    /// A seat left, on this tick.
    ///
    /// Sent by whichever machine noticed, to everyone still reachable, and
    /// folded in by [`Peer::depart`](corvid_lockstep::Peer::depart) — which
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
struct Transfer<S: State> {
    /// Which tick the state is at.
    at: Tick,
    /// The state itself.
    state: S,
    /// Every seat that has left, and when.
    departed: Vec<(u16, Tick)>,
}

/// Who has proposed what about which seat leaving, and what has been agreed.
///
/// # Why a departure needs agreeing at all
///
/// [`Peer::depart`](corvid_lockstep::Peer::depart) changes what a machine
/// simulates from the tick it names, so two machines that applied different
/// ticks would compute different states — and, because every peer sends a
/// digest of its state every tick, they would report each other as *desynced*
/// while they were merely uninformed. Keeping the earliest proposal fixes the
/// end state and not the middle: the machine that guessed later would have
/// played, sent and been judged on ticks nobody else agreed with.
///
/// So nothing is applied until everybody still here has said what they think.
/// Each machine proposes a tick, hears the others', and applies the **minimum
/// over a complete set** — which is the same number on every machine, because
/// it is the same set. Until the set is complete nothing is applied and nothing
/// diverges; the session is already stalled against the seat that stopped
/// speaking, so the wait costs what it was costing anyway.
///
/// # What it does not do
///
/// It is not a general consensus. There is no leader, no term and no proof
/// against a machine that lies — a peer that proposed one tick and simulated
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
    /// [`None`] every other time — including for a seat already agreed, because
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

/// A peer, the transport its datagrams ride on, and the seat map between them.
///
/// One tick is [`play`](Self::play): submit this machine's action, fold in
/// whatever arrived, simulate, and send. The order is the one
/// `corvid_lockstep`'s own documentation gives, and the reason it is that order
/// is that the action for `now + delay` should be in the datagram this tick
/// sends rather than in the next one.
pub(crate) struct Link<S: State> {
    /// This machine's whole lockstep state, which owns the session.
    peer: Peer<S>,
    /// What carries a datagram to the other machines.
    transport: Box<dyn Transport>,
    /// What arrived this tick, copied out of the transport's borrow so that
    /// folding one in can send an answer.
    ///
    /// A field rather than a local so that the allocation is made once for the
    /// run rather than once per tick.
    inbox: Vec<Vec<u8>>,
    /// The last datagram this peer built, encoded. Also once per run.
    outbound: Vec<u8>,
    /// What the last tick did.
    traffic: Traffic,
    /// And what all of them did.
    played: Played,
    /// Who has said what about which seat leaving.
    ///
    /// Held beside the peer rather than in it because it is about the
    /// *machines* and not about the session: what reaches the session is the
    /// one agreed tick, through [`Peer::depart`](corvid_lockstep::Peer::depart).
    departures: Departures,
    /// This machine's own proposal for each seat, kept so that it can be said
    /// again to a peer that arrives late or missed it.
    mine: BTreeMap<PlayerId, Tick>,
    /// Somewhere to leave the game's caches while a rescue borrows the peer.
    ///
    /// `Scratch` is `Default` and nothing else, so this is the one way to hand
    /// a peer method something to simulate with from a context that is not
    /// holding the run's own. It is a memo either way — a tick may not read
    /// anything out of one that its arguments do not imply — so a fresh one is
    /// always a correct one.
    /// The newest tick any peer has said it has actions up to.
    ///
    /// **This is what decides whether a stall is survivable.** A peer is stuck
    /// when the rows it is waiting for are older than the oldest row anybody
    /// still sends — and a datagram's window reaches back
    /// [`CATCHUP`](corvid_lockstep::CATCHUP) rows from its sender's head, so
    /// the comparison is this against the tick every seat has been confirmed
    /// to.
    ///
    /// It is measured in *ticks of the session* rather than in tries, which the
    /// first version of this was: a peer that is briefly ahead declines to
    /// simulate on every pass of the loop, and a loop with a `Fake` clock
    /// passes thousands of times a millisecond — so a counter of "how often did
    /// I decline" reached any threshold in microseconds of ordinary play and
    /// asked for a state transfer in the middle of a healthy session.
    heard_head: Tick,
}

impl<S: State> Link<S> {
    /// A link over `transport`, playing `seat` of `session`.
    pub(crate) fn new(
        session: Session<S>,
        seat: PlayerId,
        budget: Budget,
        transport: Box<dyn Transport>,
    ) -> Self {
        let seats = session.log.players();
        Self {
            peer: Peer::new(session, seat, budget),
            transport,
            inbox: Vec::new(),
            outbound: Vec::new(),
            traffic: Traffic::default(),
            played: Played::default(),
            departures: Departures::new(seats),
            mine: BTreeMap::new(),
            heard_head: Tick::ZERO,
        }
    }

    /// The session being played, which the peer owns.
    pub(crate) const fn session(&self) -> &Session<S> {
        &self.peer.session
    }

    /// The same, mutably, for the two things the loop does to a session that
    /// have nothing to do with the network: writing a save out of it and
    /// letting it forget its far past.
    pub(crate) const fn session_mut(&mut self) -> &mut Session<S> {
        &mut self.peer.session
    }

    /// The session, once the run is over and the peer is finished with it.
    pub(crate) fn into_session(self) -> Session<S> {
        self.peer.session
    }

    /// The state this peer is at.
    pub(crate) const fn state(&self) -> &S {
        self.peer.state()
    }

    /// The tick that state is at.
    pub(crate) const fn tick(&self) -> Tick {
        self.peer.tick()
    }

    /// What the last tick did.
    pub(crate) const fn traffic(&self) -> Traffic {
        self.traffic
    }

    /// What the whole run has done.
    pub(crate) const fn played(&self) -> Played {
        self.played
    }

    /// Opens on a state that came from somewhere other than the opening: a save
    /// slot, a recorded session, or another machine.
    ///
    /// # Errors
    ///
    /// [`Error::Halted`](crate::Error::Halted) for a tick outside the session
    /// the peer is holding.
    pub(crate) fn adopt(&mut self, at: Tick, state: S) -> Result<(), crate::Error> {
        self.peer.adopt(at, state).map_err(halted)
    }

    /// One tick: submit, receive, advance, send.
    ///
    /// The sink is the caller's, for the reason it is everywhere
    /// else in this workspace — a rollback simulates with it, so it cannot be
    /// borrowed from the loop for the length of the call — and the commands
    /// come back because [`Peer::take_commands`](corvid_lockstep::Peer::take_commands)
    /// holds what the ticks simulated for the first time asked for.
    ///
    /// # Errors
    ///
    /// [`Error::Log`](crate::Error::Log) if this machine's own action could not
    /// be recorded, and [`Error::Diverged`](crate::Error::Diverged) or
    /// [`Error::Halted`](crate::Error::Halted) for what a peer answers when the
    /// session cannot continue. **A packet that will not decode is neither**:
    /// it is counted and dropped, because a socket carries whatever is sent to
    /// it and a run that stopped on the first stray byte would be a run
    /// anybody could stop.
    pub(crate) fn play(
        &mut self,
        action: S::Action,
        command: &mut impl corvid_behavior::Command<Reference = LevelRef<S>>,
    ) -> Result<(), crate::Error> {
        let mut traffic = Traffic::default();

        // This machine's own intent, for `now + Budget::delay`. It goes in
        // before the sending below, so the datagram this tick puts on the wire
        // already carries it.
        self.peer.submit(action).map_err(refused)?;

        self.collect(&mut traffic)?;

        // Sent before simulating, which is the order `corvid_lockstep`'s own
        // worked example uses: what goes out is this seat's newest actions, and
        // they are all decided by now. Sending afterwards would put this tick's
        // simulation between the decision and the announcement of it for no
        // gain — the datagram carries no state.
        self.broadcast(&mut traffic);

        traffic.advanced = self.peer.advance(command).map_err(halted)?;

        // Stalling is ordinary: a peer declines to simulate whenever it is
        // ahead of what every seat has confirmed, and the next datagram ends
        // it. What is not ordinary is stalling for rows that no longer exist —
        // a peer whose frontier is more than a window behind the newest head
        // anybody has announced is waiting for rows that have fallen out of
        // every window still being sent, and no amount of waiting will do. It
        // says so, and whichever machine answers sends a state.
        if self.is_stuck() {
            self.say_all(Control::Stuck {
                seat: self.peer.seat().0,
                agreed: self.peer.frontier.agreed(),
            });
            tracing::warn!(
                name: "corvid_app.stuck",
                agreed = %self.peer.frontier.agreed(),
                heard = %self.heard_head,
                "the actions this machine is waiting for are older than any window                  still carries; asking for a state",
            );
        }

        self.traffic = traffic;
        self.played.fold(traffic);
        Ok(())
    }

    /// Polls the transport and folds every datagram in, deepest rollback kept.
    fn collect(&mut self, traffic: &mut Traffic) -> Result<(), crate::Error> {
        // Taken out of the transport's borrow first. `poll` hands each arrival
        // to a closure that borrows the bytes for the length of the call, and
        // what happens to a datagram here is a rollback that borrows the peer —
        // so the copy is what lets the two happen one after the other rather
        // than inside each other.
        self.inbox.clear();
        let inbox = &mut self.inbox;
        // What this poll turned up that is not a datagram, acted on after the
        // borrow ends: the sink borrows the transport, and everything below
        // borrows the peer or sends something.
        let mut gone: Vec<PeerId> = Vec::new();
        let mut heard: Vec<(PeerId, Control)> = Vec::new();
        let mut arrived: Vec<PeerId> = Vec::new();
        // Paired with the peer that sent it, because who sent a state is half
        // of whether to adopt it. See `rescue`.
        let mut transferred: Vec<(PeerId, Transfer<S>)> = Vec::new();
        self.transport.poll(&mut |from, delivery| match delivery {
            Delivery::Datagram(bytes) => inbox.push(bytes.to_vec()),
            Delivery::Stream {
                channel: Channel::Control,
                bytes,
            } => match corvid_wire::decode::<Control>(bytes) {
                Ok(control) => heard.push((from, control)),
                Err(why) => tracing::warn!(
                    name: "corvid_app.unreadable_control",
                    peer = %from,
                    why = %why,
                    "a control frame this session could not read; dropped",
                ),
            },
            Delivery::Stream {
                channel: Channel::Transfer,
                bytes,
            } => match corvid_wire::decode::<Transfer<S>>(bytes) {
                Ok(transfer) => transferred.push((from, transfer)),
                Err(why) => tracing::warn!(
                    name: "corvid_app.unreadable_transfer",
                    peer = %from,
                    why = %why,
                    "a state transfer this session could not read; dropped",
                ),
            },
            // The other reliable channels carry an opening and a state
            // transfer, and this runtime transfers no state — so a frame on one
            // is somebody else's traffic and saying so is all that can honestly
            // be done with it.
            Delivery::Stream { channel, bytes } => tracing::debug!(
                name: "corvid_app.unread_stream",
                peer = %from,
                channel = %channel,
                bytes = bytes.len(),
                "this runtime reads no reliable channel yet, so this frame is dropped",
            ),
            Delivery::Joined => {
                arrived.push(from);
                tracing::info!(
                    name: "corvid_app.peer_joined", peer = %from, "a peer is reachable",
                );
            }
            Delivery::Lost { because } => {
                gone.push(from);
                tracing::warn!(
                    name: "corvid_app.peer_lost",
                    peer = %from,
                    why = %because,
                    "a peer went away; its seat submits nothing from here on and this                      machine stops waiting for it",
                );
            }
            // `Delivery` is `#[non_exhaustive]`, so a backend built against a
            // later version of `corvid_net` may hand over something this
            // runtime has never heard of. Noting it and carrying on is the only
            // honest answer: a lockstep session's correctness rests on the
            // datagrams above, and a delivery kind that did not exist when this
            // was written cannot be one of them.
            other => tracing::debug!(
                name: "corvid_app.unknown_delivery",
                peer = %from,
                what = ?other,
                "a delivery this runtime has no handling for; dropped",
            ),
        });

        // A machine that has gone is a machine whose actions will never arrive,
        // and a peer that kept waiting for them would stall for the rest of the
        // session — which is the failure that looks like the game freezing and
        // reports nothing.
        //
        // **The tick is proposed rather than decided.** Far enough ahead that
        // no machine has confirmed past it — a peer runs at most `delay + ahead`
        // beyond what every seat has spoken for — so an ordinary departure
        // costs no rollback at all; and it is folded in by `Peer::depart`,
        // which keeps the earliest, so two machines proposing different ticks
        // land on the same one.
        self.agree(&gone, &heard, &arrived, traffic)?;
        if let Some(transfer) = self.solicited(transferred) {
            self.rescue(transfer, traffic)?;
        }

        // Out of `self` for the loop, and back at the end: folding a datagram
        // in takes `&mut self.peer`, and the buffer it is being read out of is
        // a field of the same struct. What the round trip preserves is the
        // outer allocation, which is the one made once per run.
        let mut inbox = std::mem::take(&mut self.inbox);
        for bytes in &inbox {
            let datagram: Datagram<S::Action> = match corvid_wire::decode(bytes) {
                Ok(datagram) => datagram,
                Err(why) => {
                    // Counted rather than fatal, and said at `debug` rather than
                    // `warn`: on an open socket this is ordinary. Anything may
                    // send anything to a port.
                    traffic.undecodable = traffic.undecodable.saturating_add(1);
                    tracing::debug!(
                        name: "corvid_app.undecodable",
                        bytes = bytes.len(),
                        why = %why,
                        "a datagram this session could not read; dropped",
                    );
                    continue;
                }
            };
            let rolled = self.peer.receive(&datagram).map_err(halted)?;
            traffic.heard = traffic.heard.saturating_add(1);
            let newest = datagram.head();
            if newest > self.heard_head {
                self.heard_head = newest;
            }
            if rolled.ticks > traffic.rolled.ticks {
                traffic.rolled = rolled;
            }
        }
        inbox.clear();
        self.inbox = inbox;
        Ok(())
    }

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
    fn agree(
        &mut self,
        gone: &[PeerId],
        heard: &[(PeerId, Control)],
        arrived: &[PeerId],
        traffic: &mut Traffic,
    ) -> Result<(), crate::Error> {
        let lead = u64::from(self.peer.budget.delay) + u64::from(self.peer.budget.ahead) + 1;
        let me = self.peer.seat();

        // What this machine noticed itself. The tick is far enough ahead that
        // nobody has confirmed past it — a peer runs at most `delay + ahead`
        // beyond what every seat has spoken for — so an agreement reached on it
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
            // incomplete until it does — and it is never earlier than what it
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
    fn propose(
        &mut self,
        seat: PlayerId,
        from: PlayerId,
        at: Tick,
        traffic: &mut Traffic,
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
    /// ever reach — the rows are not late, they are unsent.
    ///
    /// **A machine on its own is never stuck.** Nothing has announced anything,
    /// so there is nothing this is behind: an outage that stops every peer
    /// stops every peer's head too, and when it ends the windows still cover
    /// the gap. What produces a real one is a session that moved on without
    /// this machine in it.
    fn is_stuck(&self) -> bool {
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
    /// neither is ahead of the other — so a rule like "whoever has got further
    /// answers" leaves nobody answering, and "anybody who can, answers" has them
    /// swapping states and reopening on two different ones. One designated seat
    /// is what makes the outcome the same session on both sides, and the roster
    /// is a thing both machines already agree about.
    fn authority(&self) -> PlayerId {
        (0..self.peer.session.log.players())
            .map(PlayerId)
            .find(|seat| self.peer.departed(*seat).is_none())
            .unwrap_or_else(|| self.peer.seat())
    }

    /// Answers a peer that says it cannot catch up, with a state.
    ///
    /// Sent over [`Channel::Transfer`], which is reliable and ordered and is
    /// the one channel sized for something this big — an action datagram is a
    /// handful of bytes and this is a whole `State`.
    ///
    /// Only [`authority`](Self::authority) answers, and it answers whether or
    /// not it is ahead: what a stuck machine needs is not a *better* state but
    /// the *same* state as everybody else, and one seat deciding that is what
    /// makes it the same on every machine.
    ///
    /// **The sender reopens on its own state as well.** Otherwise it goes on
    /// waiting for rows the rescued machine will never send — they are older
    /// than the tick it just restarted at — and the session ends with one peer
    /// playing and one peer stuck, which is the failure it was fixing wearing
    /// the other hat.
    ///
    /// # Errors
    ///
    /// Whatever reopening this machine's own session reports.
    fn send_state(&mut self, to: PeerId, seat: PlayerId, agreed: Tick) -> Result<(), crate::Error> {
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
    /// end, which is the end it was missing from — any peer that cared to send
    /// a `Transfer` was obeyed.
    ///
    /// The newest wins among what is left, because the authority may have
    /// answered twice — a second `Stuck` sent while the first answer was still
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
    fn solicited(&self, transferred: Vec<(PeerId, Transfer<S>)>) -> Option<Transfer<S>> {
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
    /// its answer against one this machine derived — there is nothing to check
    /// it against, which is the whole reason a rescue exists. So the roster is
    /// the trust boundary: peers are other players, not arbitrary senders. A
    /// deployment where they are not wants authentication under
    /// [`Transport`](corvid_net::Transport), not a further test here.
    ///
    /// # Errors
    ///
    /// [`Error::Halted`](crate::Error::Halted) for a state at a tick this
    /// session cannot reach, which is one before its opening.
    fn rescue(&mut self, transfer: Transfer<S>, traffic: &mut Traffic) -> Result<(), crate::Error> {
        // The roster first. A machine that adopted the state and went on
        // simulating a seat everybody else had agreed was gone would diverge on
        // its very first tick — so the departures are applied before the state
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

    /// Tells one peer what this machine has said about every seat.
    fn tell(&self, peer: PeerId, me: PlayerId) {
        for (seat, at) in &self.mine {
            self.say(
                peer,
                Control::Departed {
                    seat: seat.0,
                    from: me.0,
                    at: *at,
                },
            );
        }
    }

    /// One control message to everybody the transport can reach.
    fn say_all(&self, control: Control) {
        for peer in self.transport.peers().get().iter() {
            self.say(peer, control);
        }
    }

    /// One control message, to one peer, reliably.
    ///
    /// Nothing here can stop the run. A control frame that will not go is a
    /// peer that has gone, and what this runtime does about a peer that has
    /// gone is the message it was trying to send.
    fn say(&self, to: PeerId, control: Control) {
        let Ok(bytes) = corvid_wire::encode(&control) else {
            tracing::error!(
                name: "corvid_app.unencodable_control",
                "a control message could not be encoded, so this peer says nothing",
            );
            return;
        };
        if let Err(why) = self.transport.send_stream(to, Channel::Control, &bytes) {
            tracing::debug!(
                name: "corvid_app.unsent_control",
                peer = %to,
                why = %why,
                "this control message did not go",
            );
        }
    }

    /// Sends this peer's newest window of actions and its digest to everyone.
    ///
    /// Nothing here can stop the run. A send that fails is a peer that has gone
    /// or a path that will not carry the frame, and a lockstep session's answer
    /// to both is the same one it has for a lost packet: predict, and correct
    /// when something arrives.
    fn broadcast(&mut self, traffic: &mut Traffic) {
        let datagram = self.peer.outgoing();
        self.outbound.clear();
        match corvid_wire::encode(&datagram) {
            Ok(bytes) => self.outbound = bytes,
            Err(why) => {
                tracing::error!(
                    name: "corvid_app.unencodable",
                    why = %why,
                    "this peer's own datagram could not be encoded, so this tick says nothing",
                );
                return;
            }
        }

        let peers = self.transport.peers().get();
        traffic.peers = u16::try_from(peers.len()).unwrap_or(u16::MAX);
        for peer in peers.iter() {
            match self.transport.send_datagram(peer, &self.outbound) {
                Ok(()) => traffic.sent = traffic.sent.saturating_add(1),
                Err(SendError::TooLarge { bytes, limit }) => tracing::error!(
                    name: "corvid_app.oversized",
                    peer = %peer,
                    bytes,
                    limit,
                    "this session's action window does not fit in one datagram, so this \
                     peer is hearing nothing from this machine",
                ),
                Err(why) => tracing::debug!(
                    name: "corvid_app.unsent",
                    peer = %peer,
                    why = %why,
                    "this tick's datagram did not go; the other end predicts through it",
                ),
            }
        }
    }
}

/// A seat map with nothing in it yet.
///
/// [`PeerId(n)`](corvid_net::PeerId) plays [`PlayerId(n)`](PlayerId), which is
/// what two peers started by the same command line have. It is a placeholder
/// that says so: a session assembled by a lobby is told who is in which seat,
/// and that mapping arrives over [`Channel::Control`](corvid_net::Channel) with
/// the roster rather than being inferred from a connection's order.
#[must_use]
pub const fn seat_of(peer: PeerId) -> PlayerId {
    PlayerId(peer.0)
}

/// The log refusing this machine's own action.
const fn refused(why: Refused) -> crate::Error {
    crate::Error::Log(why)
}

/// A peer that cannot carry on, sorted into the two things that means.
fn halted(why: Halt) -> crate::Error {
    match why {
        Halt::Desync(desync) => crate::Error::Diverged(Box::new(desync)),
        other => crate::Error::Halted(Box::new(other)),
    }
}

impl<S: State> core::fmt::Debug for Link<S> {
    /// The peer and the counters. Not the transport, which is a trait object
    /// with no `Debug` bound, and not the session, which is the run.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Link")
            .field("peer", &self.peer)
            .field("traffic", &self.traffic)
            .finish_non_exhaustive()
    }
}
