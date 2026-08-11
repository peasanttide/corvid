//! Forgetting a prefix of a trace, and of a whole session.
//!
//! The composed half of `tests/forget.rs`: a session forgets its log, its trace
//! and its snapshots together, and the property is that the result is a session
//! that seeks, saves and loads exactly as one opened later would have. A desync
//! in what was kept must still be found.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use std::sync::Arc;

use common::{Counter, forward, opening, play, schema, seats};
use corvid_hash::{Digest, digest};
use corvid_replay::{ActionLog, Forget, HashTrace, Session, Unreachable};
use corvid_time::Tick;

#[test]
fn a_forgotten_trace_keeps_its_marks_and_its_end() {
    let mut trace = HashTrace::new(Tick(4));
    for mark in 0..20 {
        trace.push(Digest::from_u64(0xfeed_0000 + mark));
    }
    let end = trace.end();

    trace.forget_before(Tick(11));

    assert_eq!(trace.first(), Tick(11));
    assert_eq!(trace.end(), end, "the frontier does not move");
    assert_eq!(trace.len(), 13);
    assert_eq!(trace.get(Tick(10)), None);
    for mark in 11..24 {
        assert_eq!(
            trace.get(Tick(mark)),
            Some(Digest::from_u64(0xfeed_0000 + mark - 4)),
            "the mark at {mark}",
        );
    }
}

#[test]
fn a_forgotten_trace_still_finds_a_desync_in_what_it_kept() {
    let mut mine = HashTrace::new(Tick::ZERO);
    let mut theirs = HashTrace::new(Tick::ZERO);
    for mark in 0..30 {
        mine.push(Digest::from_u64(mark));
        theirs.push(Digest::from_u64(if mark == 20 { 0xffff } else { mark }));
    }
    assert_eq!(mine.disagrees_with(&theirs), Some(Tick(20)));

    mine.forget_before(Tick(15));
    assert_eq!(
        mine.disagrees_with(&theirs),
        Some(Tick(20)),
        "a peer that forgot the first fifteen ticks still disagrees about the \
         twentieth",
    );

    mine.forget_before(Tick(25));
    assert_eq!(
        mine.disagrees_with(&theirs),
        None,
        "and stops being able to say anything about it once it has forgotten it",
    );
}

/// A session of `ticks` rows and the states it passed through, behind the
/// handles [`Session::forget_before`](corvid_replay::Session::forget_before)
/// and [`Session::seek`](corvid_replay::Session::seek) speak in.
///
/// A runtime is already holding the state it hands over, which is the whole
/// reason that parameter is a handle, so wrapping here rather than at nine call
/// sites is what makes these tests read the way the caller does.
fn played(ticks: u64) -> (Session<Counter>, Vec<Arc<common::State>>) {
    let session = play(ticks);
    let (states, _) = forward(&session);
    (session, states.into_iter().map(Arc::new).collect())
}

#[test]
fn a_forgotten_session_seeks_to_the_same_states() {
    let (mut session, states) = played(60);
    let last = session.last();

    let retired = session
        .forget_before(Tick(25), Arc::clone(&states[25]))
        .expect("tick 25 is inside a session that reaches tick 60");
    assert_eq!(retired, states[0], "the state the session used to open at");

    session
        .check()
        .expect("a session that forgot a prefix is still a session");
    assert_eq!(session.first(), Tick(25));
    assert_eq!(session.last(), last, "the frontier does not move");

    let mut snapshots = corvid_replay::Snapshots::new(0);
    for at in [25, 26, 41, 59, 60] {
        let (state, _) = session
            .seek(&mut snapshots, Tick(at))
            .expect("every one of these is inside what the session kept");
        assert_eq!(state, states[usize::try_from(at).unwrap()], "at tick {at}");
    }

    assert_eq!(
        session
            .seek(&mut snapshots, Tick(24))
            .map(|(state, _)| state),
        Err(Unreachable::Before {
            to: Tick(24),
            first: Tick(25),
        }),
        "and the tick before the horizon is gone rather than wrong",
    );
}

#[test]
fn a_forgotten_session_keeps_the_marks_for_what_it_kept() {
    let (mut session, states) = played(60);
    session
        .forget_before(Tick(25), Arc::clone(&states[25]))
        .expect("tick 25 is inside a session that reaches tick 60");

    assert_eq!(session.marks.first(), Tick(25));
    assert_eq!(session.marks.get(Tick(24)), None);
    for at in 25..=60 {
        assert_eq!(
            session.marks.get(Tick(at)),
            Some(digest(&states[usize::try_from(at).unwrap()])),
            "the mark at {at}",
        );
    }
}

#[test]
fn a_forgotten_session_saves_and_loads() {
    let (mut session, states) = played(60);
    session
        .forget_before(Tick(25), Arc::clone(&states[25]))
        .expect("tick 25 is inside a session that reaches tick 60");

    let bytes = session.save().expect("every part of this session encodes");
    let loaded = Session::<Counter>::load(&bytes, schema())
        .expect("a session that forgot a prefix is one this build can replay");
    assert_eq!(loaded, session);

    let (state, _) = loaded
        .seek(&mut corvid_replay::Snapshots::new(0), loaded.last())
        .expect("the last tick is always reachable");
    assert_eq!(state, states[60]);
}

#[test]
fn forgetting_to_the_tick_a_session_already_opens_at_changes_nothing() {
    let (mut session, states) = played(20);
    let untouched = session.clone();

    let retired = session
        .forget_before(Tick::ZERO, Arc::clone(&states[0]))
        .expect("a session may be told what it already knows");

    assert_eq!(retired, states[0]);
    assert_eq!(session, untouched);
}

#[test]
fn a_session_refuses_to_forget_what_it_does_not_have() {
    let (mut session, states) = played(20);
    let untouched = session.clone();

    let mut early = Session::new(opening()).expect("four seats fit in a u16");
    early.opening.first = Tick(5);
    early.log = ActionLog::new(Tick(5), seats(&session));
    early.marks = HashTrace::new(Tick(5));
    assert_eq!(
        early.forget_before(Tick(2), Arc::clone(&states[0])),
        Err(Forget::Early {
            tick: Tick(2),
            first: Tick(5),
        }),
    );

    assert_eq!(
        session.forget_before(Tick(21), Arc::clone(&states[20])),
        Err(Forget::Beyond {
            tick: Tick(21),
            last: Tick(20),
        }),
    );
    assert_eq!(session, untouched, "a refused forget changes nothing");

    // The tick the log reaches but has no row for is the one boundary worth
    // taking twice: the state there exists, so forgetting to it is legal.
    session
        .forget_before(Tick(20), Arc::clone(&states[20]))
        .expect("the last tick is a tick the session has a state for");
    assert_eq!(session.first(), Tick(20));
    assert_eq!(session.last(), Tick(20));
}
