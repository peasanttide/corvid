//! Two peers, a link between them, and a tick loop — the thing the demo runs
//! and the thing the tests assert on.
//!
//! This is deliberately *below* [`corvid`](corvid::App): a [`Match`]
//! owns two [`Peer`]s and hands their datagrams to a
//! [`MockNet`], with no clock, no window and no thread, so
//! that a session over a link that loses a third of its packets is a `for` loop
//! whose every step is decided by the seed. `tests/session.rs` is this file with
//! assertions and `src/main.rs --demo` is this file with a table printed at the
//! end; there is no third implementation of "two peers playing", which is what
//! makes the demo evidence rather than a picture.
//!
//! What plays *through* the runtime — a window, a clock, `App::transport` — is
//! `tests/linked.rs`, and it plays the same game.

use corvid::Digest;

use corvid::PlayerId;

use corvid::digest;
use corvid::{Duration, Tick};
use corvid_lockstep::{Budget, Datagram, Halt, Peer};
use corvid_net::{Delivery, MockNet, PeerId, Schedule, Transport};
use corvid_replay::Session;
use corvid_replay::Shape;

use corvid::Controller;
use serde::{Deserialize, Serialize};

use crate::{Ears, Graphics, Hands};
use crate::{Move, Table, court, opening, rules, table::SEATS};

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

/// One seat's number, as the type a peer and a player are both counted in.
#[must_use]
pub fn index(seat: usize) -> u16 {
    u16::try_from(seat).unwrap_or(u16::MAX)
}

/// What a seat's player does, tick by tick.
///
/// Two so far, and they are the two a netcode test needs: one that watches the
/// ball, so the peers disagree about the future often enough for prediction to
/// be worth testing, and one that does nothing, so a seat can be present and
/// idle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Policy {
    /// Works out where the ball is going, goes there, and decides which part of
    /// the paddle to meet it with. [`crate::bot`] is the whole of it and carries
    /// the argument.
    #[default]
    Chase,
    /// Stands still forever, which is the shape of a seat nobody is sitting in.
    Idle,
}

/// One seat, played by a [`Policy`].
///
/// A [`Controller`] rather than a function pointer, because that is what it
/// stands in for: the lab drives a [`Peer`] directly where a run drives it
/// through [`App`](corvid::App), and the thing being substituted either way is
/// the control that answers with an action per tick. As a real controller it
/// can be handed to an `App` unchanged, which is what makes the lab and a run
/// the same setup rather than two shapes that have to be kept in step.
///
/// It is a *pure function of the state this peer believes in* — which is the
/// honest thing for a test as well as for a player: it acts on what its own
/// machine is showing it, mispredictions and all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Racket {
    /// Which seat it plays, which is the paddle it moves.
    pub seat: u16,
    /// How it plays.
    pub policy: Policy,
}

impl Racket {
    /// A racket playing `seat` by `policy`.
    #[must_use]
    pub const fn new(seat: u16, policy: Policy) -> Self {
        Self { seat, policy }
    }
}

impl Controller<Table> for Racket {
    /// Itself: a seat and a policy are the whole of what one is.
    type Config = Self;

    const SETS: &'static [corvid::SetDescriptor] = crate::action::SETS;

    fn new(config: Self) -> Self {
        config
    }

    fn configure(&mut self, config: Self) {
        *self = config;
    }

    /// The input is ignored, which is the point: a scripted seat answers from
    /// the state rather than from a device.
    fn action(&self, acting: corvid::Acting<'_, Table>) -> Move {
        let seat = self.seat as usize;
        match self.policy {
            Policy::Idle => Move::Still,
            Policy::Chase => {
                let Some(paddle) = acting.state.paddles.get(seat) else {
                    return Move::Still;
                };
                crate::bot::toward(
                    paddle.at,
                    crate::bot::target(seat, acting.state, &court(), &rules()),
                    &court(),
                )
            }
        }
    }

    /// Nothing accumulates: there is no camera to smooth and no cursor to cast.
    fn update(&mut self, _updating: corvid::Updating<'_, Table>) {}

    fn look(&self) -> corvid::Camera {
        corvid::Camera::default()
    }
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
    /// there is the system working rather than failing — one of them has heard
    /// something the other has not, yet.
    pub confirmed: Tick,
    /// The digest of its state at every tick from the opening to
    /// [`tick`](Self::tick).
    ///
    /// The whole trace rather than the last one, because two peers that agree
    /// at the end may have disagreed in the middle and corrected — and a
    /// convergence test that only compared the ends would pass on a session
    /// that was wrong for four hundred ticks.
    ///
    /// **Copied out of the peer's own trace rather than accumulated here**, and
    /// the difference is not cosmetic: a rollback re-simulates ticks that
    /// already had marks, and a trace built by appending one digest per tick
    /// would keep the values from before the correction. Every one of those
    /// stale entries would compare unequal to the other peer's corrected one
    /// and read as a divergence — which is a bug in the measurement that looks
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
/// come from a seed rather than a clock — so a session over a bad link is
/// exactly reproducible, and a test that fails can be run again and fail the
/// same way.
#[derive(Debug)]
pub struct Match {
    /// The link.
    net: MockNet,
    /// The two peers, seat-indexed.
    peers: Vec<Peer<Table>>,
    /// What each seat's player does.
    policies: Vec<Racket>,
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
            // Paired with their seats here, because a seat is what a
            // policy needs to know which paddle is its own and the caller
            // already said which is which by position.
            policies: policies
                .iter()
                .enumerate()
                .map(|(seat, policy)| Racket::new(index(seat), *policy))
                .collect(),
            traces,
            period: crate::RATE.period(),
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
    /// naming a tick past the horizon. **Loss is not one of them** — a peer
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

        let endpoint = self.net.endpoint(PeerId(index(seat)));
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
            // than a condition to survive — a `MockNet` corrupts nothing — so
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
                    let _unreachable = endpoint.send_datagram(PeerId(index(other)), &bytes);
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

/// The seed the demo's link lies with, and the one `--together` plays over.
pub const SEED: u64 = 0x0f_1e_2d_3c;

/// The game [`together`] plays: the whole client half, against a peer on a
/// thread.
///
/// The same five types the binary's own game names, declared again here because
/// this mode is a *library* function a test drives and a run needs a game to be
/// told about. `()` is the bot, because the other seat is a real
/// [`Peer`] rather than something the runtime fills in.
#[cfg(feature = "window")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rallying;

#[cfg(feature = "window")]
impl corvid::Game for Rallying {
    const PERIOD: corvid::TickSpan = crate::RATE;

    type State = Table;
    type Controller = Hands;
    type Bot = ();
    type Render = Graphics;
    type Auralizer = Ears;
}

/// Plays both seats in this process: one in a window, one on a thread.
///
/// **This is real netcode against a real peer**, and the only thing about it
/// that is not two machines is that the datagrams never leave the address
/// space. The opponent below is a whole [`Peer`] — it predicts this player's
/// paddle, rolls back when the prediction is wrong, and sends its digest every
/// tick — sitting behind a [`MockNet`] on a domestic curve, so what the player
/// is playing against is a session with latency and loss in it rather than a
/// second paddle in the same simulation.
///
/// What it is *not* is an interesting opponent: it chases the ball, which is
/// [`Policy::Chase`] and is four lines. The netcode is the exhibit.
///
/// The run is handed back rather than swallowed, so that a test can read what
/// the netcode did — which is the only way to tell this mode from a single-seat
/// run with a picture of an opponent in it.
///
/// # Errors
///
/// Whatever the run answers, and [`Error::Shape`](corvid::Error::Shape) if
/// the opening cannot be made into a session.
#[cfg(feature = "window")]
pub fn together(
    seat: PlayerId,
    rate: corvid::TickSpan,
    ticks: Option<u64>,
    windowed: bool,
) -> corvid::Result<corvid::Outcome<Rallying>> {
    use corvid::{App, Error, Input};
    let net = MockNet::new(seats(), SEED);
    net.all(Schedule::DOMESTIC);

    let other = PlayerId(u16::from(seat.0 == 0));
    let opponent = net.endpoint(PeerId(other.0));
    let session = Session::new(opening()).map_err(Error::Shape)?;
    let clock = net.clone();
    let period = rate.period();
    // Detached on purpose: the window owns the process, and when it closes the
    // process ends and this goes with it. A join handle would be a promise to
    // wait for a loop with no way out of it.
    drop(std::thread::spawn(move || {
        opponent_loop(session, other, &opponent, &clock, period);
    }));

    let app = App::<Rallying>::new()
        .opening(opening())
        .rate(rate)
        .seat(seat)
        .transport(Box::new(net.endpoint(PeerId(seat.0))))
        .input(Input::new(crate::action::SETS))
        .bindings(crate::action::bindings());
    // A window is the point of this mode and not a requirement of it: without
    // one it is the same two peers with nobody watching, which is what makes it
    // something a build machine can run.
    let app = if windowed { app.window() } else { app };
    let app = match ticks {
        Some(ticks) => app.for_ticks(ticks),
        None => app,
    };
    app.run()
}

/// The opponent: one peer, one policy, and a link whose clock this drives.
///
/// It sleeps to the tick rather than spinning, and it advances the mock link's
/// clock by one period per tick — so the latency a `MockNet` schedule describes
/// passes at the same rate the game does, and the player in the window is
/// playing over a link that behaves like the one the demo table measured.
#[cfg(feature = "window")]
fn opponent_loop(
    session: Session<Table>,
    seat: PlayerId,
    endpoint: &corvid_net::Endpoint,
    net: &MockNet,
    period: Duration,
) {
    let mut peer = Peer::new(session, seat, Budget::DEFAULT);
    let mut due = std::time::Instant::now();
    loop {
        let now = std::time::Instant::now();
        if now < due {
            std::thread::sleep(due - now);
            continue;
        }
        due += period;

        let racket = Racket::new(seat.0, Policy::Chase);
        let action = racket.action(corvid::Acting {
            state: peer.state(),
            input: &corvid::Input::new(crate::action::SETS),
            time: corvid::Time {
                tick: peer.tick(),
                ..corvid::Time::default()
            },
            seat,
        });
        if peer.submit(action).is_err() {
            return;
        }

        let mut arrived: Vec<Vec<u8>> = Vec::new();
        endpoint.poll(&mut |_from, delivery| {
            if let Delivery::Datagram(bytes) = delivery {
                arrived.push(bytes.to_vec());
            }
        });
        for bytes in &arrived {
            let Ok(datagram) = corvid_wire::decode::<Datagram<Move>>(bytes) else {
                continue;
            };
            // A peer that cannot carry on stops playing. The window's own run
            // reports the same condition as an error; this thread has nobody to
            // report to, and carrying on with a halted peer would put a paddle
            // on the screen that is no longer part of the session.
            if peer.receive(&datagram).is_err() {
                return;
            }
        }
        let outgoing = peer.outgoing();
        if peer.advance(&mut corvid::Discard::new()).is_err() {
            return;
        }
        if let Ok(bytes) = corvid_wire::encode(&outgoing) {
            for other in 0..SEATS {
                if other != usize::from(seat.0) {
                    // As above: a peer that cannot be reached is predicted
                    // through rather than reported.
                    let _unreachable = endpoint.send_datagram(PeerId(index(other)), &bytes);
                }
            }
        }
        net.advance(period);
    }
}

/// The newest tick every peer in a [`Match`] has every seat's real action for,
/// which is the range two traces can honestly be compared over.
///
/// It is the minimum of two minimums: over the peers, and within each peer over
/// the seats. A mark above it was taken over a state one peer predicted part
/// of, and two predictions disagreeing is what prediction is.
#[must_use]
pub fn agreed(traces: &[Trace]) -> Tick {
    traces
        .iter()
        .map(|trace| trace.confirmed.min(trace.tick))
        .min()
        .unwrap_or(Tick::ZERO)
}
