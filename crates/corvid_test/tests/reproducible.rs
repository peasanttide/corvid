//! Every way two runs of one opening stop being one game, and the tick each is
//! reported at.
//!
//! Each test below points [`is_reproducible`] at a game that breaks in exactly
//! one way and asserts the whole answer -- the tick, what still agreed before it,
//! and both sides of the difference. Asserting only that it failed would pass
//! against a check that always fails, and asserting only the variant would pass
//! against one that names the wrong tick.
//!
//! # One spin counter per test
//!
//! The misbehaving habits read a process-global counter, and the two runs one
//! check makes have to be the first and second consumers of it for the
//! difference between them to be a known number. Tests in this binary run in
//! parallel, so each takes an index of its own: `Restless` 1, `Chatty` 2,
//! `Fickle` 3, `Halting` 4, `Blind` 5, `Fickle` again at 6 for the test about
//! the last tick, `Fleeting` 7. Index 0 is for the habits that read none.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Climb, Habit, Legs, idle, opening, rules};
use corvid_app::Command;
use corvid_behavior::PlayerId;
use corvid_test::{Diverged, Failed, What, is_reproducible};
use corvid_time::Tick;
/// How far each run plays, and the threshold the habits that have one flip at.
///
/// One run consumes exactly this many spins, so the second run starts at this
/// value: a threshold here is below every value the first run reads and at or
/// below every value the second one does.
const TICKS: u64 = 20;

/// The answer, or a failure that says which other thing went wrong.
fn diverged(answer: Result<(), Failed>) -> Diverged {
    match answer {
        Ok(()) => panic!("the two runs agreed"),
        Err(Failed::Diverged(diverged)) => diverged,
        Err(other) => panic!("the check did not get as far as a comparison: {other}"),
    }
}

#[test]
fn a_game_that_reads_nothing_it_should_not_is_reproducible() {
    // The one that has to pass. Without it every test below would be satisfied
    // by a check that returned `Err` unconditionally, and each of them asserts
    // a different `Err`.
    is_reproducible::<Climb, Legs>(
        &opening(Habit::Steady, 0, 0),
        &rules(Habit::Steady, 0, 0),
        &idle(),
        TICKS,
    )
    .unwrap();
}

#[test]
fn a_scratch_that_accumulates_is_still_reproducible() {
    // The discrimination that matters most here, because it is the one a reader
    // would guess wrong: a game whose tick reads accumulated scratch history
    // runs the same way twice, because each run starts from a fresh scratch and
    // accumulates identically. Nothing about two runs of one opening can see it
    // -- `scratch_is_a_memo_throughout` is what can, and `tests/memo.rs` is where
    // this same opening goes red.
    is_reproducible::<Climb, Legs>(
        &opening(Habit::Hoarder, 0, 0),
        &rules(Habit::Hoarder, 0, 0),
        &idle(),
        TICKS,
    )
    .unwrap();
}

#[test]
fn a_global_folded_into_the_state_is_the_first_tick_that_marks_differently() {
    let answer = is_reproducible::<Climb, Legs>(
        &opening(Habit::Restless, 1, 0),
        &rules(Habit::Restless, 1, 0),
        &idle(),
        TICKS,
    );
    let diverged = diverged(answer);

    // Tick one and not tick zero: the mark at zero is the digest of the opening
    // state, which both runs were handed rather than computed, and the first
    // state a tick produced is the state at one.
    assert_eq!(diverged.at, Tick(1));
    assert_eq!(diverged.agreed_through, Some(Tick(0)));

    let What::Marks { recorded, computed } = diverged.what else {
        panic!("{diverged}");
    };
    // Two digests and not one printed twice. A report whose two sides were the
    // same value would name a divergence nobody could act on, and the message
    // would read as the check contradicting itself.
    assert_ne!(recorded, computed);
}

#[test]
fn a_global_in_an_action_is_reported_against_the_seat_that_submitted_it() {
    let threshold = i64::try_from(TICKS).unwrap();
    let answer = is_reproducible::<Climb, Legs>(
        &opening(Habit::Fickle, 3, threshold),
        &rules(Habit::Fickle, 3, threshold),
        &idle(),
        TICKS,
    );
    let diverged = diverged(answer);

    // Tick zero, which is where the first action goes. Nothing agreed first.
    assert_eq!(diverged.at, Tick(0));
    assert_eq!(diverged.agreed_through, None);

    let What::Actions {
        seat,
        recorded,
        computed,
    } = diverged.what
    else {
        panic!("{diverged}");
    };
    assert_eq!(seat, PlayerId(0));
    // The values, not the fact that they moved. `Up` and `Leap` mean the same
    // thing to this game's tick, which is why the states agreed and why this is
    // the comparison that had to catch it.
    assert_eq!(recorded, "Some(Up)");
    assert_eq!(computed, "Some(Leap)");
}

#[test]
fn an_action_that_differs_only_on_the_last_tick_is_still_reported() {
    // The upper bound of the action comparison, which every other test here is
    // blind to: they all diverge at tick zero or one, so a comparison that
    // stopped a tick short would pass every one of them and miss the last tick
    // of every session it was ever pointed at.
    //
    // The threshold puts the flip on exactly that tick. One run consumes
    // `TICKS` spins, so the first run reads 0..TICKS and the second reads
    // TICKS..2*TICKS; a threshold of `2 * TICKS - 1` is above everything the
    // first run reads and above everything the second reads but its last.
    let threshold = i64::try_from(2 * TICKS - 1).unwrap();
    let answer = is_reproducible::<Climb, Legs>(
        &opening(Habit::Fickle, 6, threshold),
        &rules(Habit::Fickle, 6, threshold),
        &idle(),
        TICKS,
    );
    let diverged = diverged(answer);

    // The last tick the log has a row for, which is one before the tick the
    // session reaches: a row at tick `T` carries the state at `T` to the state
    // at `T + 1`.
    assert_eq!(diverged.at, Tick(TICKS - 1));
    assert_eq!(diverged.agreed_through, Some(Tick(TICKS - 2)));

    let What::Actions {
        seat,
        recorded,
        computed,
    } = diverged.what
    else {
        panic!("{diverged}");
    };
    assert_eq!(seat, PlayerId(0));
    assert_eq!(recorded, "Some(Up)");
    assert_eq!(computed, "Some(Leap)");
}

#[test]
fn a_run_that_quits_early_is_reported_as_a_reach_before_a_request() {
    let threshold = i64::try_from(TICKS).unwrap();
    let answer = is_reproducible::<Climb, Legs>(
        &opening(Habit::Halting, 4, threshold),
        &rules(Habit::Halting, 4, threshold),
        &idle(),
        TICKS,
    );
    let diverged = diverged(answer);

    let What::Reach { recorded, computed } = diverged.what else {
        panic!("{diverged}");
    };
    // The first run never reaches the threshold and plays its whole length; the
    // second starts above it and quits on its first tick, which still leaves it
    // one state past the opening.
    assert_eq!(recorded, Tick(TICKS));
    assert_eq!(computed, Tick(1));
    // The first tick one of the two has no state for, and the last one both do.
    assert_eq!(diverged.at, Tick(2));
    assert_eq!(diverged.agreed_through, Some(Tick(1)));
}

#[test]
fn a_global_in_a_request_is_reported_with_both_requests() {
    let answer = is_reproducible::<Climb, Legs>(
        &opening(Habit::Chatty, 2, 0),
        &rules(Habit::Chatty, 2, 0),
        &idle(),
        TICKS,
    );
    let diverged = diverged(answer);

    assert_eq!(diverged.at, Tick(0));
    assert_eq!(diverged.agreed_through, None);

    let What::Requested { recorded, computed } = diverged.what else {
        panic!("{diverged}");
    };
    let (recorded, computed) = (recorded.unwrap(), computed.unwrap());
    assert_eq!(recorded.tick, Tick(0));
    assert_eq!(computed.tick, Tick(0));
    // Same request, different payload, which is the shape that matters: a save
    // to a different slot and a stat with a different number are the same bug,
    // and a report that only counted requests would call these two identical.
    assert!(matches!(recorded.command, Command::Stat { .. }));
    assert_ne!(recorded.command, computed.command);
}

#[test]
fn a_field_outside_the_digest_is_caught_by_the_comparison_that_is_not_a_digest() {
    let answer = is_reproducible::<Climb, Legs>(
        &opening(Habit::Blind, 5, 0),
        &rules(Habit::Blind, 5, 0),
        &idle(),
        TICKS,
    );
    let diverged = diverged(answer);

    // Every mark agreed, so this is the last tick rather than an early one: the
    // states differed the whole way and no digest could see it.
    assert_eq!(diverged.at, Tick(TICKS));
    assert_eq!(diverged.agreed_through, Some(Tick(TICKS - 1)));
    assert!(matches!(diverged.what, What::Unequal { .. }), "{diverged}");
}

#[test]
fn a_divergence_older_than_a_default_run_would_keep_is_still_found() {
    // The check has to see the whole run, and the thing that would stop it is
    // not in this file: `App`'s retention default is a *window*, so a run that
    // inherited it would have let go of the marks for its early ticks by the
    // time it stopped, and `HashTrace::disagrees_with` compares the overlap two
    // traces have and says nothing about ticks neither still holds.
    //
    // So this run is longer than that window and its divergence is at a tick
    // well inside the part a bounded run throws away. `Retention::RECENT` is
    // 256 ticks and the run keeps between one window and two, so a bounded run
    // of `LONG` ticks opens no earlier than tick 256 -- past `AT` by a factor of
    // twenty-five.
    const LONG: u64 = 700;
    const AT: i64 = 10;

    let answer = is_reproducible::<Climb, Legs>(
        &opening(Habit::Fleeting, 7, AT),
        &rules(Habit::Fleeting, 7, AT),
        &idle(),
        LONG,
    );
    let diverged = diverged(answer);

    // The tick, and not merely that something was found. A check that compared
    // only the tail would answer `Ok`; one that compared the whole run and
    // named the wrong tick would be as useless to a reader.
    assert_eq!(diverged.at, Tick(10));
    assert_eq!(diverged.agreed_through, Some(Tick(9)));

    let What::Marks { recorded, computed } = diverged.what else {
        panic!("{diverged}");
    };
    assert_ne!(recorded, computed);
}

#[test]
fn a_run_longer_than_the_retention_window_that_reads_nothing_still_agrees() {
    // The neighbour of the test above, and the reason that one is not satisfied
    // by a check that fails whenever a run is long. Same length, same shape,
    // nothing read that should not be.
    is_reproducible::<Climb, Legs>(
        &opening(Habit::Steady, 0, 0),
        &rules(Habit::Steady, 0, 0),
        &idle(),
        700,
    )
    .unwrap();
}

#[test]
fn the_message_names_the_tick_and_both_sides() {
    // The report is the product. A `Diverged` whose fields are right and whose
    // `Display` says "the traces differ" costs a reader the same debugging
    // session as one that never found the tick.
    let answer = is_reproducible::<Climb, Legs>(
        &opening(Habit::Restless, 1, 0),
        &rules(Habit::Restless, 1, 0),
        &idle(),
        TICKS,
    );
    let message = diverged(answer).to_string();
    assert!(message.contains("tick 1"), "{message}");
    assert!(message.contains("agreed through 0"), "{message}");
    assert!(message.contains("marked"), "{message}");
}

#[test]
fn an_opening_with_no_seat_zero_is_refused_rather_than_reported_as_a_divergence() {
    // The distinction `Failed` exists for. An empty roster is a run that cannot
    // start, and folding it into `Diverged` would make it read as
    // nondeterminism -- a wrong answer to the question the caller asked.
    let mut opening = opening(Habit::Steady, 0, 0);
    opening.roster.clear();
    match is_reproducible::<Climb, Legs>(&opening, &rules(Habit::Steady, 0, 0), &idle(), TICKS) {
        Err(Failed::Refused(_)) => {}
        other => panic!("{other:?}"),
    }
}
