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

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{Action, Swarm, beat, origin, peer, push, session};
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

#[test]
fn a_rollback_deeper_than_the_budget_is_worked_off_over_the_ticks_that_follow() {
    // Two ticks of rollback, and a correction six ticks ().
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
    // one tick per advance, and the peer says so until it is ().
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
        let idle = Action::Idle;
        let roster: Vec<_> = (0..2)
            .map(|seat| PlayerState {
                id: PlayerId(seat),
                presence: Presence::Active,
                action: idle,
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
    println!("rollback budget — {rows} rows, 6 ticks, {profile} profile");
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
