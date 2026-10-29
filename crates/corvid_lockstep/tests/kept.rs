//! A seat whose machine went, kept by the machine still here: the rows
//! nobody sent are what it predicted, so it plays on without rewinding, and
//! a state it already passed is read back the same for a save.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Action, Swarm, peer, push};
use corvid_behavior::PlayerId;
use corvid_hash::digest;
use corvid_lockstep::{Budget, Datagram, Peer};

/// Whatever the test needs to say went wrong.
type Fallible = Result<(), Box<dyn std::error::Error>>;

/// How many creeps the fixture carries.
const ROWS: u32 = 4;

/// One tick: the playing peers submit, swap datagrams, and advance.
fn round(peers: &mut [Peer<Swarm>], playing: usize) -> Fallible {
    for (seat, peer) in peers.iter_mut().enumerate().take(playing) {
        peer.submit(push(i16::try_from(seat).unwrap_or(0) + 1))?;
    }
    let sent: Vec<Datagram<Action>> = peers.iter().take(playing).map(Peer::outgoing).collect();
    for (seat, peer) in peers.iter_mut().enumerate().take(playing) {
        for (from, datagram) in sent.iter().enumerate() {
            if from != seat {
                peer.receive(datagram)?;
            }
        }
    }
    for peer in peers.iter_mut().take(playing) {
        peer.advance(&mut corvid_behavior::Discard::new())?;
    }
    Ok(())
}

#[test]
fn a_kept_seat_plays_on_from_what_was_predicted_without_rewinding() -> Fallible {
    let mut peers: Vec<Peer<Swarm>> = (0..2)
        .map(|seat| peer(ROWS, 2, seat, Budget::DEFAULT))
        .collect();
    for _ in 0..20 {
        round(&mut peers, 2)?;
    }
    // Seat one's machine goes; seat zero predicts it until its budget stops
    // it, then keeps the seat.
    for _ in 0..6 {
        round(&mut peers, 1)?;
    }
    let stalled = peers[0].tick();
    let from = peers[0].take_over(PlayerId(1))?;
    assert!(peers[0].extra.contains(&PlayerId(1)));
    assert!(from >= stalled, "the kept seat's first row is behind play");
    // Kept twice is kept once.
    assert_eq!(peers[0].take_over(PlayerId(1))?, from);
    let mut finals = Vec::new();
    for _ in 0..40 {
        peers[0].submit_for(PlayerId(1), push(9))?;
        round(&mut peers, 1)?;
        assert_eq!(peers[0].depth(), 0, "keeping the seat rewound");
        let at = peers[0].tick();
        finals.push((at, digest(peers[0].state())));
    }
    assert!(
        peers[0].tick() > stalled.saturating_add(30),
        "it did not play on"
    );
    // Every state it passed is final now, and reads back the same.
    for (at, mark) in finals.iter().take(30) {
        let Some(state) = peers[0].final_state(*at) else {
            panic!("the state at {at} could not be read back");
        };
        assert_eq!(
            digest(&state),
            *mark,
            "the state at {at} read back otherwise"
        );
    }
    Ok(())
}
