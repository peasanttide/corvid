//! The two halves of a state transfer, driven by a transport that says exactly
//! what a test wants said.
//!
//! A machine that cannot catch up from actions asks for a state, and whichever
//! machine answers sends one. Both halves are ordinary code paths reached
//! through `App::transport`, and both are hard to *provoke* over a real link —
//! an outage that stalls everybody stalls nobody's head, so the window still
//! covers the gap and no transfer is needed. What genuinely needs one is a
//! machine whose session has moved on without it, which is what this scripts.
//!
//! The transport here is a fake in the useful sense: it implements the trait
//! completely and honestly, and it *decides* what has arrived rather than
//! carrying it from somewhere. That is what makes the test deterministic where
//! two threads and a link would not be.

#![cfg(feature = "net")]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::panic_in_result_fn,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use std::sync::{Arc, Mutex};

use common::{Ears, Hands, Painted, Tally, opening, resting};
use corvid_app::{App, Retention};
use corvid_behavior::PlayerId;
use corvid_hash::digest;
use corvid_net::{Channel, Delivery, PeerId, PeerSet, SendError, Transport};
use corvid_signal::Watch;
use corvid_signal::channel as watch;
use corvid_time::{Fake, Tick, TickRate};
/// Whatever the test needs to say went wrong.
type Fallible = Result<(), Box<dyn std::error::Error>>;

/// One frame this transport was asked to send.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Sent {
    /// Which peer it was for.
    to: PeerId,
    /// Which channel it went on, and [`None`] for an unreliable datagram.
    channel: Option<Channel>,
    /// The bytes.
    bytes: Vec<u8>,
}

/// One arrival a test has queued up: who it is from, the bytes, and the channel
/// it came on — [`None`] for an unreliable datagram.
type Arrival = (PeerId, Vec<u8>, Option<Channel>);

/// A transport that hands over a scripted queue and records everything sent.
#[derive(Debug)]
struct Scripted {
    /// What the next polls will produce, oldest first.
    incoming: Mutex<Vec<Arrival>>,
    /// Everything this machine tried to send.
    sent: Arc<Mutex<Vec<Sent>>>,
    /// Who is reachable.
    peers: Watch<PeerSet>,
}

impl Scripted {
    /// A transport reaching `peers`, with `incoming` waiting to be polled.
    fn new(peers: &[PeerId], incoming: Vec<Arrival>, sent: Arc<Mutex<Vec<Sent>>>) -> Self {
        let (emitter, watch) = watch("peers", peers.iter().copied().collect::<PeerSet>());
        // The roster never changes, so the emitter has nothing more to say and
        // is dropped here — the watch keeps the value it was built with.
        drop(emitter);
        Self {
            incoming: Mutex::new(incoming),
            sent,
            peers: watch,
        }
    }

    /// The lock, with a poisoned one treated as an ordinary one.
    fn queue(&self) -> std::sync::MutexGuard<'_, Vec<Arrival>> {
        self.incoming
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Transport for Scripted {
    fn send_datagram(&self, to: PeerId, bytes: &[u8]) -> Result<(), SendError> {
        self.sent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Sent {
                to,
                channel: None,
                bytes: bytes.to_vec(),
            });
        Ok(())
    }

    fn send_stream(&self, to: PeerId, channel: Channel, bytes: &[u8]) -> Result<(), SendError> {
        self.sent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Sent {
                to,
                channel: Some(channel),
                bytes: bytes.to_vec(),
            });
        Ok(())
    }

    fn poll(&self, sink: &mut dyn FnMut(PeerId, Delivery<'_>)) {
        // One arrival per poll, so a test can say "this happened on that tick"
        // rather than "this happened at some point during the run".
        let arrival = self.queue().pop();
        if let Some((from, bytes, channel)) = arrival {
            match channel {
                Some(channel) => sink(
                    from,
                    Delivery::Stream {
                        channel,
                        bytes: &bytes,
                    },
                ),
                None => sink(from, Delivery::Datagram(&bytes)),
            }
        }
    }

    fn peers(&self) -> &Watch<PeerSet> {
        &self.peers
    }
}

/// A two-seat opening, so there is somebody to talk to.
fn two_seats() -> corvid_replay::Opening<Tally> {
    let mut opening = opening::<Tally>(common::Rules::quiet());
    opening.roster.push(corvid_replay::Profile {
        account: corvid_behavior::ProfileId(1001),
        joined: Tick::ZERO,
        left: None,
    });
    opening
}

/// A run of `ticks` ticks over a scripted transport, and everything it sent.
fn play(
    seat: u16,
    ticks: u64,
    incoming: Vec<Arrival>,
) -> Result<(corvid_app::Outcome<Tally>, Vec<Sent>), Box<dyn std::error::Error>> {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let transport = Scripted::new(&[PeerId(1 - seat)], incoming, Arc::clone(&recorded));
    let outcome = App::<Tally, Hands, Painted, Ears>::new()
        .opening(two_seats())
        .seat(PlayerId(seat))
        .rate(TickRate::CRADLE)
        .clock(Fake::stepping(TickRate::CRADLE.period()))
        .transport(Box::new(transport))
        .input(resting())
        .for_ticks(ticks)
        .retain(Retention::Everything)
        .run()?;
    let frames = std::mem::take(
        &mut *recorded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    Ok((outcome, frames))
}

/// A machine that hears somebody is stuck sends a state.
///
/// It is seat zero doing the answering, which is the seat every machine in the
/// session would name: the answer has to be the same one everywhere, or two
/// peers reopen on two different states and the session ends as two sessions.
#[test]
fn a_machine_that_hears_a_stuck_peer_sends_it_a_state() -> Fallible {
    let asking = corvid_wire::encode(&Control::Stuck {
        seat: 1,
        agreed: Tick::ZERO,
    })?;
    // Eight ticks, which is inside `Budget::DEFAULT`'s reach: this machine
    // hears nobody's actions but its own, so past that it would stall waiting
    // for a seat this test never speaks for — and a run that stalls is a run
    // that never reaches its tick count.
    let (outcome, sent) = play(0, 8, vec![(PeerId(1), asking, Some(Channel::Control))])?;

    let transfers: Vec<&Sent> = sent
        .iter()
        .filter(|frame| frame.channel == Some(Channel::Transfer))
        .collect();
    assert_eq!(
        transfers.len(),
        1,
        "a peer that said it was stuck was answered {} times",
        transfers.len(),
    );

    // And what was sent is a state of this session rather than a shape that
    // happens to encode: it decodes, and the state in it is one this run
    // actually reached.
    let transfer: Handover = corvid_wire::decode(&transfers[0].bytes)?;
    assert_eq!(
        Some(digest(&transfer.state)),
        outcome.session.marks.get(transfer.at),
        "the state sent is not the one this machine's own trace records at that tick",
    );

    // **And the sender restarted there too.** That is the half of the rule that
    // is easy to leave out and impossible to notice afterwards: a machine that
    // sends a state and goes on waiting for the rows it was waiting for before
    // is a machine that has rescued somebody into a session it is itself stuck
    // in. Its session reopening at the tick it sent is what says it did not.
    assert_eq!(
        outcome.session.first(),
        transfer.at,
        "the machine that sent a state did not reopen its own session on it",
    );
    Ok(())
}

/// A machine handed a state adopts it, and its session restarts there.
///
/// The tick jumps forward, which is the whole point — the run was at tick two
/// and the state is from tick five hundred, and no window of actions could have
/// carried it there.
#[test]
fn a_machine_handed_a_state_restarts_on_it() -> Fallible {
    let at = Tick(500);
    let handed = Handover {
        at,
        state: Tally {
            count: 4_242,
            now: at,
            movers: Vec::new(),
        },
        departed: Vec::new(),
    };
    let bytes = corvid_wire::encode(&handed)?;
    let (outcome, _sent) = play(1, 12, vec![(PeerId(0), bytes, Some(Channel::Transfer))])?;

    assert!(
        outcome.state.now.0 >= at.0,
        "a run handed a state from tick {} ended at tick {}",
        at.0,
        outcome.state.now.0,
    );
    assert_eq!(
        outcome.traffic.rescues, 1,
        "the run did not count the state it was handed",
    );
    assert_eq!(
        outcome.session.first(),
        at,
        "the session was not reopened at the tick the state came from — the \
         ticks before it are ones this machine never simulated, and keeping \
         them would be keeping a trace of nothing",
    );
    assert_eq!(outcome.session.marks.get(at), Some(digest(&handed.state)));
    Ok(())
}

/// A state from a seat that does not answer for the session is dropped.
///
/// Adopting one assigns this machine's tick and its whole simulation and
/// forgets every row before them. So the question of *who sent it* is the whole
/// question, and the answer used to be nobody's: any peer that put a
/// `Transfer` on the wire was obeyed, whether this machine had asked or not.
///
/// Seat zero is the authority here, which is what makes this checkable from one
/// process: this run **is** seat zero, so a state arriving from seat one is by
/// construction one the session does not take its answers from.
#[test]
fn a_state_from_a_seat_that_does_not_answer_is_dropped() -> Fallible {
    let at = Tick(500);
    let handed = Handover {
        at,
        state: Tally {
            count: 4_242,
            now: at,
            movers: Vec::new(),
        },
        departed: Vec::new(),
    };
    let bytes = corvid_wire::encode(&handed)?;
    let ticks = 8;
    let (outcome, _sent) = play(0, ticks, vec![(PeerId(1), bytes, Some(Channel::Transfer))])?;

    assert_eq!(
        outcome.traffic.rescues, 0,
        "a state from a seat that does not answer was adopted",
    );
    assert!(
        outcome.state.now.0 <= ticks,
        "the run was carried to tick {} by a state it should have dropped",
        outcome.state.now.0,
    );
    assert_eq!(
        outcome.session.first(),
        Tick::ZERO,
        "the session was reopened by a state it should have dropped, which \
         would have thrown away every row before tick {at}",
    );
    Ok(())
}

/// A state carries the roster's departures with it.
///
/// Without them the rescued machine adopts the state and goes on simulating a
/// seat everybody else agreed was gone — which is a divergence on its very
/// first tick, and one that no digest comparison would blame on the transfer.
#[test]
fn a_state_brings_the_departures_with_it() -> Fallible {
    let at = Tick(200);
    let handed = Handover {
        at,
        state: Tally {
            count: 7,
            now: at,
            movers: Vec::new(),
        },
        departed: vec![(0, Tick(100))],
    };
    let bytes = corvid_wire::encode(&handed)?;
    let (outcome, _sent) = play(1, 12, vec![(PeerId(0), bytes, Some(Channel::Transfer))])?;

    assert_eq!(
        outcome.session.opening.roster[0].left,
        Some(Tick(100)),
        "the departure that came with the state is not in the roster",
    );
    // And the run is at the tick the state came from rather than where it was:
    // adopting the state moved it, and the departure did not stop it.
    assert_eq!(outcome.state.now, at);
    assert_eq!(outcome.traffic.rescues, 1);
    Ok(())
}

/// The wire shapes, spelled out here rather than exported from the runtime.
///
/// `corvid_app`'s control and transfer messages are private — they are a
/// protocol between two copies of one runtime, not an interface a game writes
/// against — so a test that scripts one writes it down. That is a real
/// duplication and it is the point: if the runtime's shape changes and this
/// does not, these tests fail, which is the only way a private wire format gets
/// a compatibility check at all.
#[derive(serde::Serialize, serde::Deserialize)]
enum Control {
    /// Matches `Control::Stuck`, **and is first because that one is**: a
    /// variant is encoded by its index, so the order here is part of the shape
    /// rather than a matter of taste.
    Stuck {
        /// Which seat is asking.
        seat: u16,
        /// What it has.
        agreed: Tick,
    },
    /// Matches `Control::Departed`, and is here so that the index above is the
    /// index the runtime writes.
    #[allow(dead_code, reason = "declared so that `Stuck` keeps index zero")]
    Departed {
        /// Which seat left.
        seat: u16,
        /// Which seat says so.
        from: u16,
        /// When.
        at: Tick,
    },
}

/// The same for a transfer.
#[derive(serde::Serialize, serde::Deserialize)]
struct Handover {
    /// Which tick the state is at.
    at: Tick,
    /// The state.
    state: Tally,
    /// Every seat that has left, and when.
    departed: Vec<(u16, Tick)>,
}
