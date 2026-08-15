//! What a rollback costs, and the budget that bounds it.
//!
//! The seam against `rollback.rs` is the question: that file is whether a
//! correction lands on the tick it should, and this is how much work one is
//! allowed to be and how long that work takes.

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

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{Action, Swarm, beat, peer, push};
use corvid_behavior::PlayerState;
use corvid_behavior::{PlayerId, Presence, State};
use corvid_hash::{Digest, digest};
use corvid_lockstep::{Budget, WINDOW};
use corvid_replay::Snapshots;
use corvid_time::Tick;

/// The frame a fifteen-hertz tick has to fit in.
const FRAME: Duration = Duration::from_millis(66);

/// The state the spec argues the budget against.
const CROWD: u32 = 50_000;

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
fn a_rollback_deeper_than_the_budget_is_worked_off_over_the_ticks_that_follow() {
    // Two ticks of rollback, and a correction six ticks deep.
    let budget = Budget::new(2, 2, 8);
    let mut peer = peer(32, 2, 0, budget);
    let opening = peer.session.marks.get(Tick::ZERO).unwrap();

    for at in 0..12_u64 {
        peer.submit(Action::Idle).unwrap();
        if at <= 7 && (at + 1).is_multiple_of(u64::try_from(WINDOW).unwrap()) {
            let _ = peer.receive(&quiet(at, opening)).unwrap();
        }
        let _ = peer.advance(&mut corvid_behavior::Discard::new()).unwrap();
    }
    assert_eq!(peer.tick(), Tick(12));

    // Seat 1 has been silent since tick 7, and the action it was doing at tick
    // 8 is four ticks behind where this peer has got to.
    let rolled = peer
        .receive(&beat(
            1,
            11,
            [push(9), Action::Idle, Action::Idle, Action::Idle],
            opening,
        ))
        .unwrap();

    // It rewound to the corrected tick and re-simulated as much as it had
    // budget for, rather than spending four ticks inside one frame.
    assert_eq!(rolled.from, Tick(8));
    assert_eq!(rolled.to, Tick(10));
    assert_eq!(rolled.ticks, 2);
    assert_eq!(peer.tick(), Tick(10));
    assert!(peer.stalled());

    // A visible hitch rather than a missed frame budget: the rest is worked off
    // one tick per advance, and the peer says so until it is caught up.
    for expected in [true, false] {
        let advanced = peer.advance(&mut corvid_behavior::Discard::new()).unwrap();
        assert_eq!(advanced.stalled, expected, "at tick {}", advanced.tick.0);
    }
    assert_eq!(peer.tick(), Tick(12));
}

#[test]
fn a_peer_past_its_ahead_budget_stalls_rather_than_predicting_a_decision() {
    let mut peer = peer(32, 2, 0, Budget::DEFAULT);

    // Seat 1 never says anything, so `agreed` never leaves the opening and the
    // peer may run exactly `ahead` ticks past it.
    for _ in 0..Budget::DEFAULT.ahead {
        peer.submit(Action::Idle).unwrap();
        let advanced = peer.advance(&mut corvid_behavior::Discard::new()).unwrap();
        assert!(!advanced.stalled);
    }

    let advanced = peer.advance(&mut corvid_behavior::Discard::new()).unwrap();
    assert!(advanced.stalled);
    assert_eq!(advanced.tick, Tick(u64::from(Budget::DEFAULT.ahead)));
}

/// The spec's hardest performance requirement, measured before there is a game
/// to blame it on.
///
/// Six rollback ticks over fifty thousand entities inside one 66 millisecond
/// tick. The assertion is release-only because the measurement is meaningless
/// in a build that is not optimised, and the table is printed either way with
/// the profile named.
#[test]
fn six_rollback_ticks_over_fifty_thousand_entities_fit_in_one_frame() {
    let mut peer = play(CROWD, 14, 7);
    let opening = peer.session.marks.get(Tick::ZERO).unwrap();
    // Seat 1 has been silent since tick 7. Its real action for tick 8 is not
    // the idle one this peer repeated into ticks 8 through 13, so six ticks are
    // stale.
    let correction = beat(
        1,
        11,
        [push(9), Action::Idle, Action::Idle, Action::Idle],
        opening,
    );

    // The phases, each measured on its own against the same state and the same
    // six ticks of work, so that the total below can be read as something other
    // than one number.
    let state = peer.state().clone();
    let restore = time(|| drop(peer.state().clone()));
    let mark = time(|| {
        for _ in 0..6 {
            let _ = digest(&state);
        }
    });
    let keep = {
        let mut ring = Snapshots::<Swarm>::new(64 << 20);
        time(|| {
            for at in 0..6 {
                ring.keep(&peer.session.log, Tick(at), &state);
            }
        })
    };
    let simulate = {
        let roster: Vec<_> = (0..2)
            .map(|seat| PlayerState {
                id: PlayerId(seat),
                presence: Presence::Active,
                action: Action::Idle,
            })
            .collect();
        let level = Arc::clone(&peer.session.opening.content);
        let rules = Arc::clone(&peer.session.opening.rules);
        let mut previous = state;
        time(move || {
            for _ in 0..6 {
                let next = previous.clone().tick(
                    &level,
                    &roster,
                    &rules,
                    &mut corvid_behavior::Discard::new(),
                );
                () = ();
                previous = next;
            }
        })
    };

    let began = Instant::now();
    let rolled = peer.receive(&correction).unwrap();
    let took = began.elapsed();
    report(
        CROWD,
        [
            ("restore   (clone one state)", restore),
            ("simulate  (six ticks)", simulate),
            ("digest    (six states)", mark),
            ("snapshot  (six keeps)", keep),
        ],
        took,
    );

    assert_eq!(
        rolled.ticks, 6,
        "six ticks, which is the budget's own number"
    );
    assert_eq!(peer.tick(), Tick(14));
}

/// How long a piece of work took.
fn time(work: impl FnOnce()) -> Duration {
    let began = Instant::now();
    work();
    began.elapsed()
}

/// Prints the table, names the profile, and holds the bar in release.
fn report(rows: u32, phases: [(&'static str, Duration); 4], total: Duration) {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!();
    println!("rollback budget -- {rows} rows, 6 ticks, {profile} profile");
    println!("  (each phase timed separately against the same state and the same six ticks)");
    for (phase, took) in phases {
        println!(
            "  {phase:<30} {:>9.3} ms   {:>5}",
            took.as_secs_f64() * 1e3,
            percent(took, total),
        );
    }
    println!(
        "  {:<30} {:>9.3} ms   {:>5}",
        "TOTAL     (Peer::receive)",
        total.as_secs_f64() * 1e3,
        percent(total, total),
    );
    println!(
        "  {:<30} {:>9.3} ms   {:>5} spent",
        "budget    (one 15 Hz tick)",
        FRAME.as_secs_f64() * 1e3,
        percent(total, FRAME),
    );
    println!();

    assert!(
        cfg!(debug_assertions) || total <= FRAME,
        "the rollback took {total:?}, past the {FRAME:?} a fifteen-hertz tick has",
    );
}

/// The share of one duration another took, as a printable percentage.
fn percent(took: Duration, of: Duration) -> String {
    format!("{:.1}%", took.as_secs_f64() / of.as_secs_f64() * 100.0)
}
