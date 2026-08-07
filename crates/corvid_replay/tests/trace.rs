//! One digest per tick: where a mark lands, what a rollback does to the marks
//! after it, and exactly what comparing two traces establishes.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_hash::Digest;

use corvid_replay::HashTrace;

use corvid_time::Tick;
/// A trace from `first` holding `count` marks, each one recognisable.
fn trace(first: Tick, count: u64) -> HashTrace {
    let mut trace = HashTrace::new(first);
    for mark in 0..count {
        trace.push(Digest::from_u64(0x1000 + mark));
    }
    trace
}

#[test]
fn the_first_mark_belongs_to_the_first_tick() {
    let trace = trace(Tick(40), 3);
    assert_eq!(trace.get(Tick(40)), Some(Digest::from_u64(0x1000)));
    assert_eq!(trace.get(Tick(42)), Some(Digest::from_u64(0x1002)));
    assert_eq!(trace.get(Tick(43)), None);
    assert_eq!(trace.get(Tick(39)), None);
    assert_eq!(trace.end(), Tick(43));
}

#[test]
fn a_trace_that_opens_late_does_not_answer_for_tick_zero() {
    // A trace that ignored `first` and indexed by the raw tick would answer
    // here, with the mark for tick 40.
    let trace = trace(Tick(40), 3);
    assert_eq!(trace.get(Tick::ZERO), None);
    assert_eq!(trace.get(Tick(1)), None);
}

#[test]
fn a_rollback_drops_the_marks_it_invalidates_and_keeps_the_rest() {
    let mut trace = trace(Tick(40), 5);
    trace.truncate_from(Tick(42));
    assert_eq!(trace.len(), 2);
    assert_eq!(trace.get(Tick(41)), Some(Digest::from_u64(0x1001)));
    // The tick rolled back to is itself invalidated: its state is about to be
    // recomputed against a corrected log.
    assert_eq!(trace.get(Tick(42)), None);
    assert_eq!(trace.end(), Tick(42));

    // And the trace goes on from where it was cut, rather than from the end it
    // used to have.
    trace.push(Digest::from_u64(0x2000));
    assert_eq!(trace.get(Tick(42)), Some(Digest::from_u64(0x2000)));
}

#[test]
fn rolling_back_past_the_opening_empties_the_trace() {
    let mut trace = trace(Tick(40), 5);
    trace.truncate_from(Tick(4));
    assert!(trace.is_empty());
    assert_eq!(trace.end(), Tick(40));
}

#[test]
fn two_peers_that_diverged_are_reported_at_the_first_tick_they_differ_on() {
    let mine = trace(Tick::ZERO, 6);
    let mut theirs = trace(Tick::ZERO, 6);
    theirs.truncate_from(Tick(3));
    theirs.push(Digest::from_u64(0xdead));
    theirs.push(Digest::from_u64(0xbeef));
    theirs.push(Digest::from_u64(0xcafe));

    // Ticks 0 through 2 agree, so the answer is 3 and not 0 — a check that
    // compared only the last mark, or that reported the count of differences,
    // would not name the tick to bisect from.
    assert_eq!(mine.disagrees_with(&theirs), Some(Tick(3)));
    assert_eq!(theirs.disagrees_with(&mine), Some(Tick(3)));
}

#[test]
fn a_peer_that_is_behind_is_behind_rather_than_wrong() {
    let mine = trace(Tick::ZERO, 6);
    let theirs = trace(Tick::ZERO, 2);
    assert_eq!(mine.disagrees_with(&theirs), None);
    assert_eq!(theirs.disagrees_with(&mine), None);
}

#[test]
fn only_the_overlap_is_compared() {
    // Their trace opens at tick 3 and its first mark is the one this trace
    // holds for tick 3, so the two agree. A comparison that lined the two
    // vectors up by index rather than by tick would compare their tick 3
    // against this trace's tick 0 and report a disagreement that is not there.
    let mine = trace(Tick::ZERO, 6);
    let mut theirs = HashTrace::new(Tick(3));
    theirs.push(Digest::from_u64(0x1003));
    theirs.push(Digest::from_u64(0x1004));
    assert_eq!(mine.disagrees_with(&theirs), None);

    // And when the overlap does differ, it is the overlapping tick that is
    // named rather than either trace's first.
    let mut wrong = HashTrace::new(Tick(3));
    wrong.push(Digest::from_u64(0x1003));
    wrong.push(Digest::from_u64(0xffff));
    assert_eq!(mine.disagrees_with(&wrong), Some(Tick(4)));
}

#[test]
fn traces_that_never_overlap_report_nothing_which_is_not_agreement() {
    // The one answer a caller has to read carefully, pinned so that it is a
    // documented behaviour rather than a surprise: `None` means nothing
    // compared here disagreed, and two traces about different stretches of a
    // session compare nothing at all.
    let early = trace(Tick::ZERO, 3);
    let late = trace(Tick(900), 3);
    assert_eq!(early.disagrees_with(&late), None);
    assert_eq!(late.disagrees_with(&early), None);
}

#[test]
fn an_empty_trace_agrees_with_everything() {
    let mine = trace(Tick::ZERO, 6);
    let empty = HashTrace::new(Tick::ZERO);
    assert_eq!(mine.disagrees_with(&empty), None);
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
}
