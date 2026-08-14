//! Seeking to a tick, and the snapshot ring that decides what it costs.
//!
//! That a seek lands on the same state as running forward, whatever the budget;
//! that a correction invalidates exactly the snapshots after it; and that a
//! rollback recovers what the forward run produced.
//!
//! `tests/seek_roster.rs` holds what a seek reconstructs *about the players*.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Action, Counter, forward, play, scripted};
use corvid_behavior::PlayerId;
use corvid_hash::digest;
use corvid_replay::{Session, Snapshots};
use corvid_time::Tick;

/// Enough for a handful of these states and nowhere near enough for five
/// hundred, so the ring is under pressure in every test that uses it.
const TIGHT: usize = 1 << 12;

/// Enough for every state a five-hundred-tick session produces.
const ROOMY: usize = 1 << 24;

#[test]
fn seek_reaches_the_same_state_as_running_forward() {
    let session = play(500);
    let (states, _) = forward(&session);
    let mut snapshots = Snapshots::new(ROOMY);

    // Shuffled, because a seek that only ever went forwards would pass in tick
    // order and never restore a snapshot at all.
    for tick in [137, 0, 499, 42, 500, 1, 300, 138, 7, 499, 121, 6] {
        let (state, _) = session
            .seek(&mut snapshots, Tick(tick))
            .expect("every one of these ticks is in the log");
        let expected = &states[usize::try_from(tick).unwrap()];
        assert_eq!(&*state, expected, "tick {tick}");
        // Deliberately not dereferenced on the left. `seek` returns a handle
        // and `forward` returns a value, so this is also the assertion that a
        // digest does not notice the difference -- the property the whole
        // capture format rests on now that three of an `Opening`'s fields are
        // handles too.
        assert_eq!(digest(&state), digest(expected), "tick {tick}");
    }
}

#[test]
fn seek_is_independent_of_the_snapshot_budget() {
    let session = play(500);
    let (states, _) = forward(&session);

    // Room for exactly one snapshot, and room for a hundred. The two rings
    // evict on completely different schedules, so if which snapshot a seek
    // landed on could change what it returned, these two columns would part
    // company somewhere.
    let one = {
        let mut measure: Snapshots<Counter> = Snapshots::new(usize::MAX);
        assert!(measure.keep(&session.log, Tick::ZERO, &states[499]));
        measure.charged()
    };
    let mut lean = Snapshots::new(one);
    let mut fat = Snapshots::new(one * 100);

    for tick in 0..=500 {
        let (from_lean, _) = session.seek(&mut lean, Tick(tick)).unwrap();
        let (from_fat, _) = session.seek(&mut fat, Tick(tick)).unwrap();

        assert_eq!(digest(&from_lean), digest(&from_fat), "tick {tick}");
        assert_eq!(
            *from_lean,
            states[usize::try_from(tick).unwrap()],
            "tick {tick}"
        );
    }

    // And the cost did differ, which is the other half of the claim: if the two
    // rings had ended up holding the same thing, the agreement above would be
    // evidence about one ring rather than two. The budgets are one snapshot and
    // a hundred, and that is what the two rings hold -- a hundredth of a budget
    // being spent on a single entry is what would make "a hundred" a figure
    // about the constructor rather than about the ring.
    assert_eq!(lean.len(), 1, "the lean ring held {} snapshots", lean.len());
    assert!(fat.len() >= 90, "the fat ring held {} snapshots", fat.len());
}

#[test]
fn a_seek_backwards_lands_on_a_snapshot_rather_than_the_opening() {
    // The reason the ring is worth having at all, stated as the thing a seek
    // does rather than as the shape of the ring -- `tests/snapshots.rs` reads
    // the shape.
    let session = play(500);
    let mut snapshots = Snapshots::new(TIGHT);
    let mut state = session.opening.origin();

    for tick in 0..=500 {
        snapshots.keep(&session.log, Tick(tick), &*state);
        if tick < 500 {
            let (next, _) = session.seek(&mut snapshots, Tick(tick + 1)).unwrap();
            state = next;
        }
    }

    let landed = snapshots
        .nearest(&session.log, Tick(250))
        .expect("the ring holds something at or before 250")
        .0;
    assert!(
        landed > Tick::ZERO,
        "a seek to tick 250 would replay all 250 ticks from the opening",
    );

    // And the seek that lands there still returns the right answer, which is
    // the half a spread assertion on its own would not establish.
    let (states, _) = forward(&session);
    let (from_ring, _) = session.seek(&mut snapshots, Tick(250)).unwrap();
    assert_eq!(*from_ring, states[250]);
}

/// Which seat and tick the rollback tests below correct.
const LATE: (Tick, PlayerId) = (Tick(95), PlayerId(1));

/// What the late packet turns out to have said, which is deliberately not the
/// default: a "correction" that left the stored action where it was would not be
/// one, and every assertion below would pass for the wrong reason.
const REAL: Action = Action::Bump;

/// A hundred ticks of scripted actions with one entry left open, which is the
/// shape a peer that has simulated ahead of a late packet is actually in.
///
/// The open entry is unconfirmed and reads `Action::default()`, so a seek runs
/// it as an idle seat -- the prediction -- and the packet that arrives later is a
/// [`ActionLog::set`](corvid_replay::ActionLog::set) that changes it. That is a
/// correction the log can count; replacing the whole log with a differently
/// built one is not, and is what [`Snapshots::clear`] is for.
fn awaiting_a_late_packet() -> Session<Counter> {
    let mut session = Session::new(common::opening()).unwrap();
    session.log.extend_to(Tick(99)).unwrap();
    for tick in 0..100 {
        for seat in 0..common::seats(&session) {
            let player = PlayerId(seat);
            if (Tick(tick), player) == LATE {
                continue;
            }
            session
                .log
                .set(Tick(tick), player, scripted(Tick(tick), player))
                .unwrap();
        }
    }
    session
}

#[test]
fn a_rollback_recovers_the_forward_result() {
    // State to 100 against a prediction at 95, take the real action, seek
    // back, and re-simulate. The answer has to equal a clean run that had the
    // real action all along, and it has to differ from the run that predicted --
    // otherwise the whole test is about a tick where the two did the same thing.
    let mut session = awaiting_a_late_packet();
    let (mispredicted, _) = forward(&session);

    let mut snapshots = Snapshots::new(ROOMY);
    let (before, _) = session.seek(&mut snapshots, Tick(100)).unwrap();
    assert_eq!(digest(&before), digest(&mispredicted[100]));
    drop(before);

    // The packet arrives. The log counts it, because it changed a stored action.
    let generation = session.log.generation();
    session.log.set(LATE.0, LATE.1, REAL).unwrap();
    assert_eq!(session.log.generation(), generation + 1);

    let (clean, _) = forward(&session);
    assert_ne!(
        digest(&clean[100]),
        digest(&mispredicted[100]),
        "the correction has to change the answer for this to be a rollback test",
    );

    // Giving back the budget the invalidated snapshots are holding is still
    // worth doing -- they are charged until something takes them -- but it is not
    // what makes the answer right. The test below is the same rollback without
    // it.
    snapshots.discard_from(LATE.0);

    let (rolled, _) = session.seek(&mut snapshots, Tick(95)).unwrap();
    assert_eq!(digest(&rolled), digest(&clean[95]));

    let (recovered, _) = session.seek(&mut snapshots, Tick(100)).unwrap();
    assert_eq!(digest(&recovered), digest(&clean[100]));
    assert_eq!(*recovered, clean[100]);
}

#[test]
fn a_stale_snapshot_is_not_returned_when_the_ring_is_not_discarded_from() {
    // The hazard this pins: a snapshot at tick 100 taken before a correction and
    // handed straight back after it would put the correction in the log and not
    // in the answer. The log carries a generation and every entry in the ring
    // records the one it was taken under, so the seek skips the entry and
    // re-simulates instead -- without being told to.
    let mut session = awaiting_a_late_packet();
    let (mispredicted, _) = forward(&session);

    let mut snapshots = Snapshots::new(ROOMY);
    let (_before, _) = session.seek(&mut snapshots, Tick(100)).unwrap();
    assert!(snapshots.ticks().any(|tick| tick == Tick(100)));

    session.log.set(LATE.0, LATE.1, REAL).unwrap();
    let (clean, _) = forward(&session);
    assert_ne!(digest(&clean[100]), digest(&mispredicted[100]));

    // The ring still holds the entry at tick 100, and the seek does not use it.
    let (answer, _) = session.seek(&mut snapshots, Tick(100)).unwrap();
    assert_eq!(digest(&answer), digest(&clean[100]));
    assert_ne!(digest(&answer), digest(&mispredicted[100]));
}

#[test]
fn a_correction_invalidates_the_snapshots_after_it_and_leaves_the_earlier_ones() {
    // The half the test above does not cover, and the half that decides whether
    // the generation is worth having: throwing away the snapshots *before* a
    // correction would be safe and would also make every rollback replay from
    // the opening.
    //
    // The row at tick 95 carries the state at 95 to the state at 96, so the
    // states at 95 and earlier were built without ever reading it and are
    // untouched. What a correction there invalidates is 96 onwards.
    let mut session = awaiting_a_late_packet();
    let mut snapshots: Snapshots<Counter> = Snapshots::new(ROOMY);
    let (predicted, _) = forward(&session);
    for tick in [50_usize, 94, 95, 96, 100] {
        let at = Tick(u64::try_from(tick).unwrap());
        snapshots.keep(&session.log, at, &predicted[tick]);
    }

    session.log.set(LATE.0, LATE.1, REAL).unwrap();

    // Still reachable, because nothing they were built from moved.
    for tick in [50, 94, 95] {
        assert_eq!(
            snapshots
                .nearest(&session.log, Tick(tick))
                .map(|(at, _)| at),
            Some(Tick(tick)),
            "the snapshot at {tick} was thrown away by a correction at 95",
        );
    }

    // And no longer reachable, because they were.
    for tick in [96, 100] {
        assert_eq!(
            snapshots
                .nearest(&session.log, Tick(tick))
                .map(|(at, _)| at),
            Some(Tick(95)),
            "the snapshot at {tick} survived a correction at 95",
        );
    }

    // The ring is still holding all five: the generation decides what a seek may
    // use, and `discard_from` is what gives the memory back.
    assert_eq!(snapshots.len(), 5);
}

#[test]
fn ordinary_play_does_not_invalidate_the_snapshot_it_has_just_kept() {
    // Why the generation counts corrections at rows *before* a tick rather than
    // at it. A runtime keeps the state at tick `S` and only then learns what the
    // seats did on tick `S`; if writing row `S` invalidated the snapshot at `S`,
    // every entry in the ring would go stale one tick after it was taken and the
    // ring would be worth nothing. Counting row `S` would be the safe-looking
    // choice and is the one that empties the ring.
    let mut session = Session::new(common::opening()).unwrap();
    session.log.extend_to(Tick(49)).unwrap();
    let mut snapshots = Snapshots::new(ROOMY);
    let mut state = session.opening.origin();

    for tick in 0..50 {
        // The state at `tick`, kept before the row at `tick` has arrived.
        snapshots.keep(&session.log, Tick(tick), &*state);
        for seat in 0..common::seats(&session) {
            let player = PlayerId(seat);
            session
                .log
                .set(Tick(tick), player, scripted(Tick(tick), player))
                .unwrap();
        }
        let (next, _) = session.seek(&mut snapshots, Tick(tick + 1)).unwrap();
        state = next;
    }

    // Every snapshot the run kept is still one the log agrees with, even though
    // the log took a correction on every one of those fifty ticks.
    assert!(session.log.generation() > 50);
    assert_eq!(
        snapshots.nearest(&session.log, Tick(50)).map(|(at, _)| at),
        Some(Tick(50)),
    );
    for tick in [1, 17, 49] {
        assert_eq!(
            snapshots
                .nearest(&session.log, Tick(tick))
                .map(|(at, _)| at),
            Some(Tick(tick)),
            "the snapshot at {tick} went stale during ordinary forward play",
        );
    }
}
