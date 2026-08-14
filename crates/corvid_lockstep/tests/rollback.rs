//! The rollback, the generation rule it turns on, and the budget the whole
//! design is measured against.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::similar_names,
    reason = "two peers of one session are `here` and `there`, and the datagrams between them are named after them"
)]
#![allow(
    clippy::print_stdout,
    reason = "the budget measurement is a number a person has to read, and a test that measured it without printing it would be a number nobody has"
)]

mod common;

use common::{Action, Swarm, beat, origin, peer, push, session};
use corvid_hash::{Digest, digest};
use corvid_lockstep::{Budget, WINDOW};
use corvid_replay::Snapshots;
use corvid_time::Tick;

/// Four seat-1 actions, all idle, for the four ticks ending at `head`.
fn quiet(head: u64, mark: Digest) -> corvid_lockstep::Datagram<Action> {
    beat(1, head, [Action::Idle; WINDOW], mark)
}

/// Drives one peer forward to `to`, confirming seat 1 as idle through
/// `confirmed` and submitting seat 0's own actions all the way.
///
/// Seat 1 goes quiet after `confirmed`, which is what leaves the ticks after it
/// predicted and therefore rollable-().
fn play(rows: u32, to: u64, confirmed: u64) -> corvid_lockstep::Peer<Swarm> {
    let mut peer = peer(rows, 2, 0, Budget::DEFAULT);
    let opening = peer.session.marks.get(Tick::ZERO).unwrap();

    for at in 0..to {
        peer.submit(if at % 5 == 0 { push(3) } else { Action::Idle })
            .unwrap();
        // A datagram covers four rows, so one every four ticks confirms seat 1
        // contiguously and keeps `agreed` within the peer's `ahead` budget.
        if at <= confirmed && (at + 1).is_multiple_of(u64::try_from(WINDOW).unwrap()) {
            let _ = peer.receive(&quiet(at, opening)).unwrap();
        }
        let advanced = peer.advance(&mut corvid_behavior::Discard::new()).unwrap();
        assert!(!advanced.stalled, "the peer stalled on the way to {to}");
    }
    assert_eq!(peer.tick(), Tick(to));
    peer
}

#[test]
fn discard_from_is_inclusive_of_the_tick_it_is_given() {
    // The one thing the rollback rule rests on, asserted against `Snapshots`
    // itself rather than inferred: `discard_from(T)` drops the entry *at* `T`.
    // So a correction for tick `T`, which leaves the state at `T` untouched and
    // invalidates every state after it, is `discard_from(T.next())`.
    let mut session = session(4, 1);
    session.log.extend_to(Tick(10)).unwrap();
    let mut snapshots = Snapshots::<Swarm>::new(1 << 20);
    for at in 0..=10 {
        snapshots.keep(&session.log, Tick(at), &origin(4));
    }

    snapshots.discard_from(Tick(5));

    assert_eq!(
        snapshots.ticks().collect::<Vec<_>>(),
        (0..5).map(Tick).collect::<Vec<_>>(),
    );
}

#[test]
fn a_mispredict_at_forty_rolls_a_peer_at_forty_six_back_six_ticks() {
    let mut peer = play(64, 46, 39);
    let opening = peer.session.marks.get(Tick::ZERO).unwrap();

    // Seat 1 was silent from tick 40, so every tick from there repeated its
    // idle action from 39. It was building.
    let rolled = peer
        .receive(&beat(
            1,
            43,
            [push(9), Action::Idle, Action::Idle, Action::Idle],
            opening,
        ))
        .unwrap();

    assert_eq!(rolled.from, Tick(40));
    assert_eq!(rolled.to, Tick(46));
    assert_eq!(rolled.ticks, 6, "41 through 46 re-simulated");
    assert_eq!(peer.tick(), Tick(46), "and it is () where it was");
    assert_eq!(peer.depth(), 6);
}

#[test]
fn the_snapshot_at_the_corrected_tick_survives_the_rollback() {
    let mut peer = play(64, 46, 39);
    let opening = peer.session.marks.get(Tick::ZERO).unwrap();
    assert_eq!(
        peer.snapshots
            .nearest(&peer.session.log, Tick(40))
            .map(|(at, _)| at),
        Some(Tick(40)),
        "the ring held tick 40 before the correction",
    );

    peer.receive(&beat(
        1,
        43,
        [push(9), Action::Idle, Action::Idle, Action::Idle],
        opening,
    ))
    .unwrap();

    // The state *at* 40 is what the rows *before* 40 produce, and the
    // correction is to the row *at* 40. Counting row 40 would have taken this
    // entry, and every entry the ring ever holds, and sent the seek () to the
    // opening.
    assert_eq!(
        peer.snapshots
            .nearest(&peer.session.log, Tick(40))
            .map(|(at, _)| at),
        Some(Tick(40)),
    );
    assert!(
        peer.snapshots.ticks().all(|at| at <= Tick(46)),
        "and nothing after the tick it rolled forward to is left",
    );
}

#[test]
fn a_lossy_peer_reaches_the_state_a_perfect_one_reaches() {
    /// How many ticks one peer's inbound datagrams are held for.
    const LAG: usize = 5;
    /// How long the session runs.
    const TICKS: usize = 60;

    let mut here = peer(32, 2, 0, Budget::DEFAULT);
    let mut there = peer(32, 2, 1, Budget::DEFAULT);
    let mut delayed = Vec::new();

    for at in 0..TICKS {
        let at = u64::try_from(at).unwrap();
        here.submit(if at.is_multiple_of(5) {
            push(3)
        } else {
            Action::Idle
        })
        .unwrap();
        there
            .submit(if at.is_multiple_of(7) {
                push(-2)
            } else {
                Action::Idle
            })
            .unwrap();

        let (from_here, from_there) = (here.outgoing(), there.outgoing());
        // The perfect link: `there` learns what `here` did at once.
        let _ = there.receive(&from_here).unwrap();

        // The lossy one: `here` learns what `there` did five ticks late, so
        // every one of those ticks was predicted and some of them wrongly.
        delayed.push(from_there);
        if delayed.len() > LAG {
            let old = delayed.remove(0);
            let _ = here.receive(&old).unwrap();
        }

        let _ = here.advance(&mut corvid_behavior::Discard::new()).unwrap();
        let _ = there.advance(&mut corvid_behavior::Discard::new()).unwrap();
    }

    // Drain the link, then let both catch up to the same tick.
    for old in delayed {
        let _ = here.receive(&old).unwrap();
    }
    for _ in 0..(LAG * 4) {
        let _ = here.advance(&mut corvid_behavior::Discard::new()).unwrap();
        let _ = there.advance(&mut corvid_behavior::Discard::new()).unwrap();
    }

    assert_eq!(here.tick(), there.tick());
    assert_eq!(
        digest(here.state()),
        digest(there.state()),
        "the peer that predicted and rolled () computed what the peer that \
         never had to computed",
    );
    for at in 0..=here.tick().0 {
        assert_eq!(
            here.session.marks.get(Tick(at)),
            there.session.marks.get(Tick(at)),
            "the traces disagree at tick {at}",
        );
    }
}
