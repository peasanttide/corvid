//! What happens to a session when somebody closes their window.
//!
//! The case this file exists for is **three machines**, because two cannot
//! disagree: with one opponent left there is nobody to compute a different
//! state from. With three, two survivors notice a departure at different
//! moments -- one is a tick ahead, one heard the transport a poll later -- and if
//! each stopped waiting on its own schedule they would simulate different
//! rosters and diverge. That is the failure this crate must not have, and this
//! is where it is checked.

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
use corvid_time::Tick;
/// Whatever the test needs to say went wrong.
type Fallible = Result<(), Box<dyn std::error::Error>>;

/// How many creeps the fixture carries. Four is enough for a state that a
/// mispredicted action visibly changes.
const ROWS: u32 = 4;

/// Hands every peer's newest datagram to every other peer.
fn exchange(peers: &mut [Peer<Swarm>]) -> Fallible {
    let sent: Vec<Datagram<Action>> = peers.iter().map(Peer::outgoing).collect();
    for (seat, peer) in peers.iter_mut().enumerate() {
        for (from, datagram) in sent.iter().enumerate() {
            if from != seat {
                peer.receive(datagram)?;
            }
        }
    }
    Ok(())
}

/// One tick on every peer: submit, exchange, advance.
fn round(peers: &mut [Peer<Swarm>], acting: &[bool]) -> Fallible {
    for (seat, peer) in peers.iter_mut().enumerate() {
        if acting.get(seat).copied().unwrap_or(false) {
            peer.submit(push(i16::try_from(seat).unwrap_or(0) + 1))?;
        }
    }
    exchange(peers)?;
    for peer in peers.iter_mut() {
        peer.advance(&mut corvid_behavior::Discard::new())?;
    }
    Ok(())
}

/// Three peers, one leaves, and the two survivors apply the tick they agreed.
///
/// The agreement itself is the runtime's -- `corvid_app::Departures` is where a
/// set of opinions becomes one number, and its own tests are where that is
/// checked. What is checked here is the half this crate owns: two machines
/// handed the same tick simulate the same session from it.
#[test]
fn two_survivors_applying_one_agreed_tick_reach_one_state() -> Fallible {
    let mut peers: Vec<Peer<Swarm>> = (0..3)
        .map(|seat| peer(ROWS, 3, seat, Budget::DEFAULT))
        .collect();

    for _ in 0..20 {
        round(&mut peers, &[true, true, true])?;
    }
    // Seat two stops. The other two carry on predicting it until their budgets
    // stop them, which is the stall a departure exists to end.
    for _ in 0..4 {
        round(&mut peers, &[true, true, false])?;
    }

    // The agreed tick, applied by both. It is ahead of everything anybody has
    // simulated -- which is what a runtime proposing `tick + delay + ahead`
    // guarantees -- so it costs neither of them a rollback.
    let agreed = Tick(40);
    for peer in &mut peers[..2] {
        let rolled = peer.depart(PlayerId(2), agreed)?;
        assert!(
            !rolled.happened(),
            "an agreed departure ahead of play rewound"
        );
    }

    for _ in 0..60 {
        round(&mut peers, &[true, true, false])?;
    }

    assert!(
        peers[0].tick() > agreed,
        "the session did not reach the departure"
    );
    assert_eq!(peers[0].tick(), peers[1].tick());
    assert_eq!(
        digest(peers[0].state()),
        digest(peers[1].state()),
        "two peers that applied the same departure reached different states",
    );
    Ok(())
}

/// A departure corrected to an earlier tick leaves the session where it would
/// have been had that tick been known all along.
///
/// **This is the claim that makes the correction safe rather than merely
/// possible.** A runtime with `corvid_app::Departures` in front of it does not
/// produce a peer holding the wrong tick -- a machine waiting on the agreement
/// is still waiting on the seat that went, so it cannot run past anything -- but
/// a save written before the agreement, and a state transferred from a machine
/// that agreed something else, both can. So the crate has to answer the same
/// state either way, and here it is asked both ways.
#[test]
fn a_correction_lands_where_the_right_answer_always_was() -> Fallible {
    let agreed = Tick(24);

    // The machine that was told late: it believed a later tick, played past the
    // real one, and is then corrected.
    let mut corrected = peer(ROWS, 2, 0, Budget::DEFAULT);
    for _ in 0..10 {
        corrected.submit(push(1))?;
        corrected.advance(&mut corvid_behavior::Discard::new())?;
    }
    corrected.depart(PlayerId(1), Tick(48))?;
    for _ in 0..40 {
        corrected.submit(push(1))?;
        corrected.advance(&mut corvid_behavior::Discard::new())?;
    }
    let overshot = corrected.tick();
    assert!(
        overshot > agreed,
        "the peer did not get past the tick it is corrected to"
    );

    let rewound = corrected.depart(PlayerId(1), agreed)?;
    assert!(
        rewound.happened(),
        "correcting a departure to an earlier tick did not roll anything (),          which would leave the ticks played with the wrong roster standing",
    );

    // The machine that knew from the start.
    let mut knew = peer(ROWS, 2, 0, Budget::DEFAULT);
    for _ in 0..10 {
        knew.submit(push(1))?;
        knew.advance(&mut corvid_behavior::Discard::new())?;
    }
    knew.depart(PlayerId(1), agreed)?;

    // Both played to the same tick, and the corrected one is working off a
    // rollback deeper than its budget a tick at a time.
    while corrected.tick() < overshot || knew.tick() < overshot {
        if corrected.tick() < overshot {
            corrected.submit(push(1))?;
            corrected.advance(&mut corvid_behavior::Discard::new())?;
        }
        if knew.tick() < overshot {
            knew.submit(push(1))?;
            knew.advance(&mut corvid_behavior::Discard::new())?;
        }
    }

    assert_eq!(corrected.tick(), knew.tick());
    assert_eq!(
        digest(corrected.state()),
        digest(knew.state()),
        "a session corrected to a departure reached a different state from one          that knew about it all along",
    );
    Ok(())
}

/// The fold rule, on its own: earliest wins, repeats do nothing.
///
/// The runtime hands this one agreed tick, so in an ordinary session it is
/// never called twice for a seat -- but a state transfer, a save and a
/// retransmitted control frame can all say the same thing again, and none of
/// them may move a departure a session has already simulated through.
#[test]
fn a_departure_only_ever_moves_earlier() -> Fallible {
    let mut alone = peer(ROWS, 2, 0, Budget::DEFAULT);
    alone.depart(PlayerId(1), Tick(50))?;
    assert_eq!(alone.departed(PlayerId(1)), Some(Tick(50)));

    // Later: refused, and nothing is rolled ().
    let rolled = alone.depart(PlayerId(1), Tick(70))?;
    assert!(!rolled.happened());
    assert_eq!(alone.departed(PlayerId(1)), Some(Tick(50)));

    // The same again: nothing at all.
    let rolled = alone.depart(PlayerId(1), Tick(50))?;
    assert!(!rolled.happened());

    // Earlier: taken.
    alone.depart(PlayerId(1), Tick(30))?;
    assert_eq!(alone.departed(PlayerId(1)), Some(Tick(30)));

    // And a seat this session does not have is not an error, because a
    // transport can name a peer no roster seats.
    alone.depart(PlayerId(9), Tick(30))?;
    assert_eq!(alone.departed(PlayerId(9)), None);
    Ok(())
}

/// A session with nobody left to hear from still moves.
///
/// The reason `Peer::depart` exists: without it the frontier waits on a seat
/// that will never speak, every peer stalls at `Budget::ahead` past it, and the
/// game stops with nothing reporting why.
#[test]
fn a_session_carries_on_after_a_departure() -> Fallible {
    let mut peers: Vec<Peer<Swarm>> = (0..2)
        .map(|seat| peer(ROWS, 2, seat, Budget::DEFAULT))
        .collect();

    for _ in 0..10 {
        round(&mut peers, &[true, true])?;
    }
    let stalled_at = peers[0].tick();

    // Seat one goes silent and nobody says anything about it.
    for _ in 0..40 {
        round(&mut peers[..1], &[true])?;
    }
    let waited = peers[0].tick();
    assert!(
        waited.0 <= stalled_at.0 + u64::from(Budget::DEFAULT.ahead) + 1,
        "a peer with a silent seat ran to tick {} rather than stalling near {}",
        waited.0,
        stalled_at.0,
    );

    // And once the departure is agreed, it plays on.
    peers[0].depart(PlayerId(1), waited.saturating_add(2))?;
    for _ in 0..40 {
        round(&mut peers[..1], &[true])?;
    }
    assert!(
        peers[0].tick().0 > waited.0 + 30,
        "a peer whose opponent has left is still stalled at tick {}",
        peers[0].tick().0,
    );
    Ok(())
}

/// A machine too far behind to be caught up by actions is caught up by a
/// state.
///
/// **This is the case a window cannot fix.** A datagram carries at most
/// [`CATCHUP`](corvid_lockstep::CATCHUP) rows, so a link that was down for
/// longer than that leaves a hole no retransmission reaches: the peer stalls,
/// safely and for ever. What ends it is a whole state from a machine that has
/// one -- and afterwards the two play on together, which is the assertion.
#[test]
fn a_peer_beyond_catching_up_is_rescued_by_a_state() -> Fallible {
    let mut ahead = peer(ROWS, 2, 0, Budget::DEFAULT);
    let mut behind = peer(ROWS, 2, 1, Budget::DEFAULT);

    // The one that is ahead plays a long way alone. Its opponent's seat is
    // departed so that it is free to, which is the shape of a machine whose
    // link went away -- and it goes further than any window could carry.
    ahead.depart(PlayerId(1), Tick(2))?;
    for _ in 0..(corvid_lockstep::CATCHUP as u64 * 4) {
        ahead.submit(push(1))?;
        ahead.advance(&mut corvid_behavior::Discard::new())?;
    }
    let at = ahead.tick();
    assert!(
        at.0 > corvid_lockstep::CATCHUP as u64 * 2,
        "the gap is not wider than a window, so this tests nothing",
    );

    // The state, as a transfer would carry it: the tick, the state, and the
    // roster's departures -- which the receiver needs as much as the state,
    // because a roster it disagreed about would be a session it diverged from
    // on the first tick.
    behind.depart(PlayerId(1), Tick(2))?;
    // `resync` rather than `adopt`, and the difference is the whole point: this
    // state is from *ahead* of where the rescued peer has been, so there is no
    // trace to correct -- the session is reopened there instead.
    behind.resync(at, ahead.state().clone())?;

    assert_eq!(behind.tick(), at);
    assert_eq!(
        digest(behind.state()),
        digest(ahead.state()),
        "the adopted state is not the state that was sent",
    );

    // And from there they play the same session. Seat one is departed, so the
    // rescued peer submits nothing and follows.
    for _ in 0..40 {
        ahead.submit(push(1))?;
        let sent = ahead.outgoing();
        behind.receive(&sent)?;
        ahead.advance(&mut corvid_behavior::Discard::new())?;
        behind.advance(&mut corvid_behavior::Discard::new())?;
    }
    assert_eq!(behind.tick(), ahead.tick());
    assert_eq!(
        digest(behind.state()),
        digest(ahead.state()),
        "a rescued peer diverged from the machine that rescued it",
    );
    Ok(())
}

/// A departure is part of the session, so a replay reproduces it.
///
/// This is what makes the tick agreed rather than remembered: it is written
/// into the roster, which is what a save carries and what
/// [`Session::seek`](corvid_replay::Session::seek) rebuilds a roster from.
#[test]
fn a_departure_is_in_the_session_and_replays() -> Fallible {
    let mut peers: Vec<Peer<Swarm>> = (0..2)
        .map(|seat| peer(ROWS, 2, seat, Budget::DEFAULT))
        .collect();
    for _ in 0..12 {
        round(&mut peers, &[true, true])?;
    }
    peers[0].depart(PlayerId(1), Tick(20))?;
    for _ in 0..30 {
        round(&mut peers[..1], &[true])?;
    }

    let played = peers[0].tick();
    assert_eq!(
        peers[0].session.opening.roster[1].left,
        Some(Tick(20)),
        "the departure is not in the roster the session carries",
    );

    // Replayed from the opening with no peer in sight, through the session's
    // own log and roster.
    let mut snapshots = corvid_replay::Snapshots::<Swarm>::new(1 << 20);
    let (state, _replayed) = peers[0].session.seek(&mut snapshots, played)?;
    assert_eq!(
        digest(&*state),
        digest(peers[0].state()),
        "replaying a session with a departure in it did not reach the state the \
         peer played to",
    );
    Ok(())
}
