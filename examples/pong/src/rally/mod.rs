//! Two peers, a link between them, and a tick loop -- the netcode lab this
//! example's claims are measured in.
//!
//! This is deliberately *below* [`corvid`](corvid::App): a [`Match`]
//! owns two [`Peer`]s and hands their datagrams to a
//! [`MockNet`], with no clock, no window and no thread, so
//! that a session over a link that loses a third of its packets is a `for` loop
//! whose every step is decided by the seed. `tests/session.rs` is this file with
//! assertions on.
//!
//! What plays *through* the runtime -- a window, a clock, `App::transport` -- is
//! [`together`] and `tests/linked.rs`, and both play the same game.

use corvid::Digest;

use corvid::PlayerId;

use corvid::digest;
use corvid::{Duration, Tick};
use corvid_lockstep::{Budget, Datagram, Halt, Peer};
use corvid_net::{Delivery, PeerId, Transport};
use corvid_net_mock::{MockNet, Schedule};
use corvid_replay::Session;
use corvid_replay::Shape;

use corvid::Controller;

use crate::{Ears, Graphics, Hands};
use crate::{Move, Table, opening, table::SEATS};

/// How many seats a match has, as the width a [`PeerId`] is counted in.
///
/// A function rather than a cast, because the workspace treats a narrowing cast
/// as something to be argued for: this game has two seats and the conversion
/// cannot lose anything, and saying so once is cheaper than saying it at every
/// call.
#[must_use]
pub fn seats() -> u16 {
    u16::try_from(SEATS).unwrap_or(u16::MAX)
}

/// One seat's number, as the type a player is counted in.
#[must_use]
pub fn index(seat: usize) -> u16 {
    u16::try_from(seat).unwrap_or(u16::MAX)
}

/// The peer that plays a seat.
///
/// Not the same number: a seat is counted from nought and a peer from one,
/// because [`PeerId(0)`](corvid_net::PeerId::NONE) is nobody. This defers to
/// [`corvid::peer_of`] rather than adding one here, so the harness and the
/// runtime cannot drift apart about which machine is which -- a link built on
/// the wrong half of that convention comes up, carries datagrams and is heard
/// by nobody, which is a session that hangs rather than one that fails.
#[must_use]
pub fn peer_at(seat: usize) -> PeerId {
    corvid::peer_of(PlayerId(index(seat)))
}

/// What one peer did over a whole session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trace {
    /// Which seat it played.
    pub seat: PlayerId,
    /// The tick it finished on.
    pub tick: Tick,
    /// The newest tick every seat has spoken for, from this peer's point of
    /// view.
    ///
    /// **This is the line the traces can be compared across.** A state at or
    /// below it was computed from actions every seat really submitted; a state
    /// above it was computed partly from a prediction, so two peers disagreeing
    /// there is the system working rather than failing -- one of them has heard
    /// something the other has not, yet.
    pub confirmed: Tick,
    /// The digest of its state at every tick from the opening to
    /// [`tick`](Self::tick).
    ///
    /// The whole trace rather than the last one, because two peers that agree
    /// at the end may have disagreed in the middle and corrected -- and a
    /// convergence test that only compared the ends would pass on a session
    /// that was wrong for four hundred ticks.
    ///
    /// **Copied out of the peer's own trace rather than accumulated here**, and
    /// the difference is not cosmetic: a rollback re-simulates ticks that
    /// already had marks, and a trace built by appending one digest per tick
    /// would keep the values from before the correction. Every one of those
    /// stale entries would compare unequal to the other peer's corrected one
    /// and read as a divergence -- which is a bug in the measurement that looks
    /// exactly like the bug it is measuring for.
    pub marks: Vec<Digest>,
    /// How many datagrams it folded in.
    pub heard: u32,
    /// How many rollbacks it did.
    pub rollbacks: u32,
    /// The deepest one, in ticks.
    pub deepest: u8,
    /// How many ticks it re-simulated in total.
    pub resimulated: u32,
    /// How many ticks it declined to simulate, because it was
    /// [`Budget::ahead`] past the tick every seat had confirmed.
    pub stalls: u32,
}

impl Trace {
    /// The digest at a tick, or [`None`] for one this peer never reached.
    #[must_use]
    pub fn mark(&self, at: Tick) -> Option<Digest> {
        self.marks.get(usize::try_from(at.0).ok()?).copied()
    }
}

/// Two peers, the link between them, and everything the run measured.
///
/// The link is a [`MockNet`], which is a real implementation of
/// [`Transport`] whose latency, jitter, loss and reorder
/// come from a seed rather than a clock -- so a session over a bad link is
/// exactly reproducible, and a test that fails can be run again and fail the
/// same way.
#[derive(Debug)]
pub struct Match {
    /// The link.
    net: MockNet,
    /// The two peers, seat-indexed.
    peers: Vec<Peer<Table>>,
    /// What each seat's player does, seat-indexed.
    policies: Vec<Policy>,
    /// What each peer has done so far.
    traces: Vec<Trace>,
    /// How long a tick is, for the link's clock. The link measures latency in
    /// real time and this loop has no clock, so the loop tells it.
    period: Duration,
}

impl Match {
    /// Two peers at the opening, over a link following `schedule`.
    ///
    /// # Errors
    ///
    /// [`Shape`] if the opening cannot be made into a session, which for this
    /// game's own opening cannot happen and is reported rather than unwrapped
    /// because this is a library.
    pub fn new(schedule: Schedule, seed: u64, policies: [Policy; SEATS]) -> Result<Self, Shape> {
        let net = MockNet::new(seats(), seed);
        net.all(schedule);

        let mut peers = Vec::with_capacity(SEATS);
        let mut traces = Vec::with_capacity(SEATS);
        for seat in 0..SEATS {
            let session = Session::new(opening())?;
            let first = session.first();
            let peer = Peer::new(session, PlayerId(index(seat)), Budget::DEFAULT);
            traces.push(Trace {
                seat: peer.seat(),
                tick: first,
                confirmed: first,
                marks: vec![digest(peer.state())],
                heard: 0,
                rollbacks: 0,
                deepest: 0,
                resimulated: 0,
                stalls: 0,
            });
            peers.push(peer);
        }

        Ok(Self {
            net,
            peers,
            // By position, which is the seat: what tells a policy which paddle
            // is its own is the seat on the `Acting` it is handed, and the seat
            // a peer is asking for is its own.
            policies: policies.to_vec(),
            traces,
            period: <Rallying as corvid::Game>::PERIOD.period(),
        })
    }

    /// The link, for a test that wants to cut it, restore it, or read its
    /// tally.
    #[must_use]
    pub const fn net(&self) -> &MockNet {
        &self.net
    }

    /// What each peer has done.
    #[must_use]
    pub fn traces(&self) -> &[Trace] {
        &self.traces
    }

    /// The state one peer believes in.
    #[must_use]
    pub fn state(&self, seat: usize) -> Option<&Table> {
        self.peers.get(seat).map(Peer::state)
    }

    /// Plays `ticks` ticks on every peer.
    ///
    /// One iteration is one tick *for each peer*, in seat order, and then the
    /// link's clock moves by one tick's period. Both peers therefore see the
    /// same wall time, and which datagrams have arrived when is a function of
    /// the schedule and the seed alone.
    ///
    /// # Errors
    ///
    /// [`Halt`] from any peer: a divergence, a contradiction, or a datagram
    /// naming a tick past the horizon. **Loss is not one of them** -- a peer
    /// that hears nothing predicts, and a peer that has predicted as far as its
    /// budget allows stalls and is counted.
    pub fn play(&mut self, ticks: u64) -> Result<(), Halt> {
        for _ in 0..ticks {
            for seat in 0..self.peers.len() {
                self.step(seat)?;
            }
            self.net.advance(self.period);
        }
        Ok(())
    }

    /// One peer's tick: decide, submit, receive, advance, send.
    ///
    /// The same order [`corvid`](corvid::App::transport)'s runtime uses,
    /// because it is the order the crate documents and because the action for
    /// `now + delay` should ride in the datagram this tick sends.
    fn step(&mut self, seat: usize) -> Result<(), Halt> {
        let Some(policy) = self.policies.get(seat).copied() else {
            return Ok(());
        };
        // Read before the peer is borrowed mutably for the rest of the
        // function, which is the whole of why it is a local.
        let seats = self.peers.len();
        let Some(peer) = self.peers.get_mut(seat) else {
            return Ok(());
        };

        let action = policy.action(corvid::Acting {
            state: peer.state(),
            input: &corvid::Input::new(crate::action::SETS),
            time: corvid::Time {
                tick: peer.tick(),
                ..corvid::Time::default()
            },
            seat: peer.seat(),
        });
        // A refusal here is the log declining this machine's own action, which
        // is a `Halt::Refused` in the same family as everything else that stops
        // a peer.
        peer.submit(action)?;

        let endpoint = self.net.endpoint(peer_at(seat));
        let mut arrived: Vec<Vec<u8>> = Vec::new();
        endpoint.poll(&mut |_from, delivery| {
            if let Delivery::Datagram(bytes) = delivery {
                arrived.push(bytes.to_vec());
            }
        });

        let mut heard = 0_u32;
        let mut rollbacks = 0_u32;
        let mut deepest = 0_u8;
        let mut resimulated = 0_u32;
        for bytes in &arrived {
            // A datagram that will not decode is a fault in this harness rather
            // than a condition to survive -- a `MockNet` corrupts nothing -- so
            // it is skipped and the run carries on, and the test that would
            // have caught it is the digest comparison.
            let Ok(datagram) = corvid_wire::decode::<Datagram<Move>>(bytes) else {
                continue;
            };
            let rolled = peer.receive(&datagram)?;
            heard += 1;
            if rolled.happened() {
                rollbacks += 1;
                deepest = deepest.max(rolled.ticks);
                resimulated += u32::from(rolled.ticks);
            }
        }

        // Sent before simulating, which is the order the crate's own example
        // uses: everything the datagram carries is decided by now.
        let outgoing = peer.outgoing();

        // A `Discard`: this harness is measuring the netcode, and what a tick
        // asked the platform for is not what it is measuring.
        let advanced = peer.advance(&mut corvid::Discard::new())?;
        let tick = peer.tick();

        if let Ok(bytes) = corvid_wire::encode(&outgoing) {
            for other in 0..seats {
                if other != seat {
                    // A send that fails is a peer that is not reachable, which
                    // is what a cut link is and is not an error: the other end
                    // predicts through it.
                    let _unreachable = endpoint.send_datagram(peer_at(other), &bytes);
                }
            }
        }

        // The peer's own trace, taken whole. It is the thing a rollback
        // rewrites, so reading it is the only way to see the corrected values.
        let marks = (0..=tick.0)
            .map(|at| peer.session.marks.get(Tick(at)).unwrap_or_default())
            .collect();
        let confirmed = peer.frontier.agreed();

        if let Some(trace) = self.traces.get_mut(seat) {
            trace.heard += heard;
            trace.rollbacks += rollbacks;
            trace.deepest = trace.deepest.max(deepest);
            trace.resimulated += resimulated;
            if advanced.stalled {
                trace.stalls += 1;
            }
            trace.marks = marks;
            trace.tick = tick;
            trace.confirmed = confirmed;
        }
        Ok(())
    }
}

/// The seed a lab link lies with, and the one [`together`] plays over.
pub const SEED: u64 = 0x0f_1e_2d_3c;

corvid::game! {
    /// The game [`together`] plays: the whole client half, against a peer on a
    /// thread.
    ///
    /// The same five types the binary's own game names, declared again here
    /// because this mode is a *library* function a test drives and a run needs
    /// a game to be told about. `()` is the bot, because the other seat is a
    /// real [`Peer`] rather than something the runtime fills in -- and the
    /// period is named here so that the loop in [`Match`] and the run in
    /// [`together`] cannot disagree about how long a tick is.
    pub struct Rallying;
    const PERIOD: corvid::TickSpan = corvid::TickSpan::from_millis(33);
    type State = Table;
    type Controller = Hands;
    type Bot = ();
    type Render = Graphics;
    type Auralizer = Ears;
}

mod policy;
mod together;

pub use policy::Policy;
pub use together::{agreed, together};
