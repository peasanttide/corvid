//! The same game through the runtime: two `App`s, two threads, one link.
//!
//! `tests/session.rs` drives two [`Peer`](corvid_lockstep::Peer)s directly,
//! which is exact and says nothing about whether a *game* can be played that
//! way. This says the other half: `App::transport` is the only line that
//! differs from a single-seat run, `State` and `Present` are untouched, and
//! two runtimes started against one `MockNet` reach the same digest.
//!
//! It is deliberately a thread each rather than a stepped loop, because that is
//! how a game is actually run — two processes, two clocks, nobody taking turns
//! — and because the claim worth testing is that the netcode does not need
//! anybody to take turns.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "every test here returns a `Result` so that a failure reaches for `?` rather than unwrapping, and asserts as well — a failed assertion in a test is a failed test, which is what a test is for"
)]

use std::{thread, time::Duration};

use corvid::Input;
use corvid::digest;
use corvid::{App, Outcome};

use corvid::PlayerId;

use corvid::{Clock, Tick};
use corvid_net::{MockNet, PeerId, Schedule, Transport};
use pong::{Hands, Move, RATE, Table, action, opening};

/// Whatever the test needs to say went wrong.
type Fallible = Result<(), Box<dyn std::error::Error>>;

/// How long each peer plays.
const TICKS: u64 = 400;

/// A transport that moves the link's clock along with the run.
///
/// A [`MockNet`] measures latency in wall time and has no clock of its own, so
/// somebody has to tell it that time has passed. In a stepped test that is the
/// loop; here there is no loop, so it is this: every poll advances the link by
/// one tick's period, which is one poll per tick per peer.
///
/// It is a wrapper rather than a change to `MockNet` because this is a property
/// of *this harness* — a real socket needs nobody to advance anything, which is
/// exactly what `tests/socket.rs` demonstrates.
#[derive(Debug)]
struct Ticking {
    /// The endpoint everything is forwarded to.
    endpoint: corvid_net::Endpoint,
    /// The link whose clock this moves.
    net: MockNet,
    /// How far to move it per poll.
    period: Duration,
}

impl Transport for Ticking {
    fn send_datagram(&self, to: PeerId, bytes: &[u8]) -> Result<(), corvid_net::SendError> {
        self.endpoint.send_datagram(to, bytes)
    }

    fn send_stream(
        &self,
        to: PeerId,
        channel: corvid_net::Channel,
        bytes: &[u8],
    ) -> Result<(), corvid_net::SendError> {
        self.endpoint.send_stream(to, channel, bytes)
    }

    fn poll(&self, sink: &mut dyn FnMut(PeerId, corvid_net::Delivery<'_>)) {
        self.net.advance(self.period);
        self.endpoint.poll(sink);
    }

    fn peers(&self) -> &corvid::Watch<corvid_net::PeerSet> {
        self.endpoint.peers()
    }
}

impl Drop for Ticking {
    /// Cuts this peer's links when its run ends, which is what a process
    /// exiting looks like from the other machine.
    ///
    /// **Without it these tests hang**, and the hang is the honest behaviour of
    /// the thing being tested rather than a flaw in the harness: a peer whose
    /// opponent has silently stopped sending waits for actions that will never
    /// arrive, for ever. What ends the wait is being *told* — `Delivery::Lost`,
    /// which a real socket produces from a goodbye or a timeout and which a
    /// `MockNet` produces from `cut`.
    fn drop(&mut self) {
        let me = self.endpoint.peer();
        for other in 0..self.net.peers() {
            if PeerId(other) != me {
                self.net.cut(me, PeerId(other), corvid_net::Lost::Closed);
            }
        }
    }
}

/// What one seat holds down, tick by tick.
///
/// A function of the tick alone, so both peers' input is decided before either
/// runs and the session's outcome does not depend on which thread got there
/// first. The two seats hold different patterns, which is what makes each one's
/// prediction of the other wrong often enough to matter.
fn pressing(seat: u16) -> impl FnMut(Tick) -> Input {
    move |at| {
        let mut input = Input::new(action::SETS);
        let period = if seat == 0 { 11 } else { 7 };
        let held = if at.0 % period < period / 2 {
            action::UP
        } else {
            action::DOWN
        };
        input.set_digital(held, corvid::Digital::HELD);
        input
    }
}

/// Plays one seat over one endpoint, headless, at this game's rate.
fn play(seat: u16, transport: Box<dyn Transport>) -> Result<Outcome<Table>, corvid::Error> {
    App::<Table, Hands>::new()
        .opening(opening())
        .rate(RATE)
        .seat(PlayerId(seat))
        // A fake clock stepping one period per reading, which is what every
        // headless run in this workspace uses: the peer's own budget is what
        // keeps the two runs together, and it is the thing being tested.
        .clock(Clock::stepping(RATE.period()))
        .transport(transport)
        .input(Input::new(action::SETS))
        .inputs(pressing(seat))
        .for_ticks(TICKS)
        .retain(corvid::Retention::Everything)
        .run()
}

/// Both runtimes play the same session and agree about every tick they both
/// confirmed.
///
/// The digests are compared over the overlap rather than at the last tick,
/// because two peers on their own clocks stop at slightly different places —
/// which is the honest shape of two machines and is exactly what the comparison
/// has to tolerate without tolerating a divergence.
#[test]
fn two_runtimes_over_one_link_agree() -> Fallible {
    let net = MockNet::new(2, 0x51_a7_e5);
    net.all(Schedule::DOMESTIC);
    let period = RATE.period();

    // Both spawned before either is joined. `.map(spawn).map(join)` reads the
    // same and is not: iterators are lazy, so it starts one peer, waits for it
    // to finish, and only then starts the other — which is a peer with nobody
    // to play against, stalling for ever against a frontier that cannot move.
    let handles: Vec<_> = (0..2_u16)
        .map(|seat| {
            let transport = Ticking {
                endpoint: net.endpoint(PeerId(seat)),
                net: net.clone(),
                period,
            };
            thread::spawn(move || play(seat, Box::new(transport)))
        })
        .collect();

    let mut outcomes = Vec::new();
    for handle in handles {
        match handle.join() {
            Ok(outcome) => outcomes.push(outcome?),
            Err(_) => return Err("a peer's thread panicked".into()),
        }
    }

    let (Some(here), Some(there)) = (outcomes.first(), outcomes.get(1)) else {
        return Err("a run produced no outcome".into());
    };

    // Where each run's *state* got to, which is not where its log got to: input
    // delay writes rows for ticks nobody has simulated yet, so a session's last
    // row is `Budget::delay` ahead of its last state. The game counts its own
    // ticks, so this is the honest number on both sides.
    let (mine, theirs) = (here.state.now, there.state.now);
    // Below the tail each peer may still have been predicting. Two runs stop
    // where they stop, and the newest few ticks of each were simulated partly
    // from a guess about what the other player did — so comparing those would
    // be asserting that two machines agree about something neither has been
    // told yet. `Budget::DEFAULT` reaches ten ticks past the confirmed line;
    // twenty is that with room.
    let overlap = Tick(mine.min(theirs).0.saturating_sub(20));
    assert!(
        overlap.0 >= TICKS / 2,
        "the two runs barely overlapped: {} and {}",
        mine.0,
        theirs.0,
    );
    let mut compared = 0_u64;
    for at in 0..=overlap.0 {
        let (Some(mine), Some(theirs)) = (
            here.session.marks.get(Tick(at)),
            there.session.marks.get(Tick(at)),
        ) else {
            continue;
        };
        assert_eq!(
            mine, theirs,
            "the two runtimes disagree at tick {at}, having agreed for {compared} ticks",
        );
        compared += 1;
    }
    assert!(
        compared >= TICKS / 2,
        "only {compared} ticks had a digest on both sides",
    );

    // And the state each run ended holding is a state of the same session:
    // whichever got further, the other's last state is in its trace, at the
    // tick that state says it is.
    let (ahead, behind) = if mine >= theirs {
        (here, there)
    } else {
        (there, here)
    };
    // The same rule again: compared at a tick both of them had every action
    // for, rather than at the last one either reached.
    assert_eq!(
        ahead.session.marks.get(overlap),
        behind.session.marks.get(overlap),
        "the two peers' traces disagree at tick {}, which both had confirmed",
        overlap.0,
    );
    Ok(())
}

/// A run with no transport is exactly the run it was before this feature
/// existed.
///
/// The single-seat path is the one every other example in this workspace uses,
/// and adding a network to the runtime is not allowed to have moved a digest.
#[test]
fn a_run_with_no_transport_is_unchanged() -> Fallible {
    let alone = |seat: u16| {
        App::<Table, Hands>::new()
            .opening(opening())
            .rate(RATE)
            .seat(PlayerId(seat))
            .clock(Clock::stepping(RATE.period()))
            .input(Input::new(action::SETS))
            .inputs(pressing(seat))
            .for_ticks(200)
            .retain(corvid::Retention::Everything)
            .run()
    };

    let once = alone(0)?;
    let twice = alone(0)?;
    assert_eq!(digest(&*once.state), digest(&*twice.state));
    assert_eq!(once.session.last(), Tick(200));

    // The seat that is not this client's submits `Move::Still` forever, because
    // nothing fills its column — which is what a single-seat run *means*, and
    // what a transport is for.
    assert_eq!(
        once.session.log.get(Tick(10), PlayerId(1)),
        Some(&Move::Still)
    );
    Ok(())
}

/// Two peers in one process reach the same state as one peer simulating both
/// seats' actions.
///
/// This is the strongest form of "the network changed nothing": the action log
/// is the whole input to the simulation, so a session assembled from two
/// machines' datagrams must produce the state a single machine holding both
/// columns produces. It is checked here by replaying the networked session's
/// own log through `Session::seek`, which is the runtime's replay path and
/// knows nothing about peers.
#[test]
fn a_networked_session_replays_to_the_same_state() -> Fallible {
    let net = MockNet::new(2, 0x9e_ed_1e);
    net.all(Schedule::MOBILE);
    let period = RATE.period();

    // Both spawned before either is joined. `.map(spawn).map(join)` reads the
    // same and is not: iterators are lazy, so it starts one peer, waits for it
    // to finish, and only then starts the other — which is a peer with nobody
    // to play against, stalling for ever against a frontier that cannot move.
    let handles: Vec<_> = (0..2_u16)
        .map(|seat| {
            let transport = Ticking {
                endpoint: net.endpoint(PeerId(seat)),
                net: net.clone(),
                period,
            };
            thread::spawn(move || play(seat, Box::new(transport)))
        })
        .collect();

    let mut outcomes = Vec::new();
    for handle in handles {
        match handle.join() {
            Ok(outcome) => outcomes.push(outcome?),
            Err(_) => return Err("a peer's thread panicked".into()),
        }
    }
    let Some(here) = outcomes.first() else {
        return Err("a run produced no outcome".into());
    };

    // Replayed from the opening through the log the session ended up holding,
    // with no peer, no transport and no prediction in sight. The snapshot ring
    // is empty, so the seek starts at the opening and simulates every row.
    // **Not the last tick.** A run stops where it stops, and the newest few
    // ticks of a peer's state were simulated partly from predictions — actions
    // the other machine had not sent yet. A seek predicts nothing: a row nobody
    // filled is `Action::default()`, which is what the *session* says happened.
    // So the two agree everywhere the peer had every seat's real action, and
    // the tail is where they are allowed to differ.
    //
    // Twenty ticks is comfortably past `Budget::DEFAULT`'s eight ahead and two
    // of delay, which is how far above the confirmed line a peer can be.
    let settled = Tick(here.state.now.0.saturating_sub(20));
    assert!(
        settled.0 > 0,
        "the run was too short to have a settled tail"
    );

    let mut snapshots = corvid_replay::Snapshots::<Table>::new(1 << 20);
    let (state, _replayed) = here.session.seek(&mut snapshots, settled)?;
    assert_eq!(
        Some(digest(&state)),
        here.session.marks.get(settled),
        "replaying the session's own log did not reproduce the trace this peer          recorded while it was playing",
    );
    Ok(())
}
