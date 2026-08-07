//! Letting go of the far past: what a session keeps, what it loses, and what
//! still works afterwards.
//!
//! The claim under test is that forgetting a prefix costs a session its reach
//! backwards and costs it nothing else. Every test here is written against a
//! second source of truth rather than against the operation itself: the entries
//! are compared against a table taken before the forget, the whole log is
//! compared against a log that only ever held the rows that are left, and the
//! states are compared against `common::forward`, which shares no code with
//! `seek`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use std::sync::Arc;

use common::{Action, Counter, forward, opening, play, schema, scripted, seats};
use corvid_behavior::PlayerId;
use corvid_hash::{Digest, digest};
use corvid_replay::{ActionLog, Forget, HashTrace, Session, Unreachable};
use corvid_time::Tick;
/// The tick every test here forgets to.
///
/// Thirteen rows of four seats is fifty-two entries, which is not a whole number
/// of bytes. A confirmation bitmap that was drained a byte at a time rather than
/// shifted a bit at a time passes at every multiple of eight and fails here,
/// which is why the boundary is this one rather than a round number.
const HORIZON: Tick = Tick(13);

/// Whether seat `player` has confirmed its action for `tick`, in the mixed
/// pattern the bitmap tests are written against.
///
/// A log where everything is confirmed cannot tell a bitmap that was shifted
/// correctly from one that was shifted by rows instead of by entries, because
/// every bit it could land on holds the same value. This leaves about a third of
/// the entries unconfirmed and does it without a period of eight, so a shift
/// that is out by any number of bits moves a boundary somebody can see.
fn speaks(tick: Tick, player: PlayerId) -> bool {
    let mixed = tick
        .0
        .wrapping_mul(0xd6e8_feb8_6659_fd93)
        .wrapping_add(u64::from(player.0).wrapping_mul(0x9e37_79b9));
    (mixed >> 41) % 3 != 0
}

/// A log of `ticks` rows from `first`, with [`scripted`] actions in the entries
/// [`speaks`] confirms and nothing in the rest.
fn mixed(first: Tick, ticks: u64, players: u16) -> ActionLog<Action> {
    let mut log = ActionLog::new(first, players);
    if ticks == 0 {
        return log;
    }
    log.extend_to(Tick(first.0 + ticks - 1))
        .expect("the log grows from its own first tick");
    for row in 0..ticks {
        let tick = Tick(first.0 + row);
        for seat in 0..players {
            let player = PlayerId(seat);
            if speaks(tick, player) {
                log.set(tick, player, scripted(tick, player))
                    .expect("a fresh log has nothing confirmed to contradict");
            }
        }
    }
    log
}

#[test]
fn every_retained_entry_reads_exactly_as_it_did() {
    let mut log = mixed(Tick::ZERO, 40, 4);
    let before: Vec<(Tick, PlayerId, Action, bool)> = (0..40)
        .flat_map(|row| {
            (0..4).map(move |seat| {
                let (tick, player) = (Tick(row), PlayerId(seat));
                (tick, player, scripted(tick, player), speaks(tick, player))
            })
        })
        .collect();

    log.forget_before(HORIZON);

    assert_eq!(log.first(), HORIZON);
    assert_eq!(log.last(), Tick(40), "the frontier does not move");
    assert_eq!(log.ticks(), 27);

    for (tick, player, action, confirmed) in before {
        if tick < HORIZON {
            assert_eq!(log.get(tick, player), None, "{tick} is before the horizon");
            continue;
        }
        // An unconfirmed entry holds the default, which is what it held before
        // the forget and is not what `scripted` would have put there.
        let expected = if confirmed { action } else { Action::default() };
        assert_eq!(log.get(tick, player), Some(&expected), "at {tick}");
        assert_eq!(
            log.is_confirmed(tick, player),
            confirmed,
            "the confirmation bit for seat {} at {tick}",
            player.0,
        );
    }
}

#[test]
fn a_log_that_forgot_a_prefix_is_a_log_that_never_held_it() {
    let mut forgotten = mixed(Tick::ZERO, 40, 4);
    forgotten.forget_before(HORIZON);

    // Built from the same script over the rows that are left, and never
    // extended past them. This compares the parts `get` and `is_confirmed`
    // cannot reach: the bitmap byte for byte, which is where a shift that moved
    // by rows rather than by entries shows up.
    let fresh = mixed(HORIZON, 27, 4);

    assert_eq!(forgotten, fresh);
    assert_eq!(forgotten.entries(), fresh.entries());
    assert_eq!(forgotten.confirmed_bytes(), fresh.confirmed_bytes());
}

/// The four fields an [`ActionLog`] is written down as, so that a test can
/// write one down that nothing in this crate would ever produce.
///
/// The names, the order and the types are the log's own, which is what makes
/// this decode as one: `corvid_wire` is `bincode`, so a struct is its fields in
/// order and the generation the log skips is not among them.
#[derive(serde::Serialize)]
struct HandMade {
    /// The tick the first row belongs to.
    first: Tick,
    /// How many seats wide a row is.
    players: u16,
    /// The entries, row-major.
    actions: Vec<Action>,
    /// The confirmation bitmap, however many bits of it are set.
    confirmed: Vec<u8>,
}

#[test]
fn a_capture_with_bits_set_past_its_entries_is_cleaned_by_a_forget() {
    // The one case the high-bit mask at the end of `forget_confirmations`
    // exists for, and it is not the shift: the shift brings zeros down into the
    // bits it vacates, so a log built by this crate's own constructors is
    // already clean above its last entry. A *decoded* log is not. `Deserialize`
    // takes the bitmap verbatim and `Session::check` compares its length
    // against the entries rather than its contents, so a corrupt or hand-made
    // capture can carry bits past its last entry through every check there is.
    //
    // Fifteen entries is one bit short of two whole bytes, so bit fifteen is
    // exactly such a bit — and forgetting one row of three shifts by three,
    // which lands it inside the last byte of what is left rather than past the
    // end of the bitmap, where the truncation would have taken it anyway.
    const SEATS: u16 = 3;
    const ROWS: usize = 5;
    let capture = |confirmed: Vec<u8>| HandMade {
        first: Tick::ZERO,
        players: SEATS,
        actions: vec![Action::Bump; ROWS * usize::from(SEATS)],
        confirmed,
    };
    let load = |made: &HandMade| -> ActionLog<Action> {
        corvid_wire::decode(&corvid_wire::encode(made).expect("a hand-made capture encodes"))
            .expect("and decodes as the log it is shaped like")
    };

    let mut clean = load(&capture(vec![0b1010_1101, 0b0010_1010]));
    let mut dirty = load(&capture(vec![0b1010_1101, 0b1010_1010]));

    // The two really do differ, and only above the last entry: every bit a
    // reader can reach agrees, and the bitmaps do not.
    assert_ne!(
        clean, dirty,
        "the doctored capture is the same as the clean one"
    );
    for row in 0..ROWS {
        for seat in 0..SEATS {
            let (tick, seat) = (Tick(row as u64), PlayerId(seat));
            assert_eq!(
                clean.is_confirmed(tick, seat),
                dirty.is_confirmed(tick, seat),
                "the confirmation for seat {} at {tick} was doctored too",
                seat.0,
            );
        }
    }

    clean.forget_before(Tick(1));
    dirty.forget_before(Tick(1));

    assert_eq!(
        clean, dirty,
        "a forget left a bit set past the last entry, so a log that came off a \
         disk is unequal to the same session played",
    );
}

#[test]
fn a_correction_a_retained_row_carries_is_still_carried() {
    let mut log = mixed(Tick::ZERO, 40, 4);
    // A row inside the stretch that survives, and one inside the stretch that
    // does not. Only the first is a correction a retained state depends on.
    log.set(Tick(20), PlayerId(0), Action::Reset)
        .expect("seat zero said nothing at tick 20");
    log.set(Tick(3), PlayerId(1), Action::Reset)
        .expect("seat one said nothing at tick 3");

    let before: Vec<(Tick, u64)> = (14..=41)
        .map(|row| (Tick(row), log.generation_at(Tick(row))))
        .collect();
    let counts: Vec<u64> = before.iter().map(|(_, count)| *count).collect();
    assert!(
        counts.first() != counts.last(),
        "the fixture has to take a correction inside the retained stretch, or \
         this test compares a column of one number against itself"
    );

    log.forget_before(HORIZON);

    for (tick, generation) in before {
        assert_eq!(log.generation_at(tick), generation, "at {tick}");
    }

    // The horizon itself is the one tick whose count moves, and it moves to
    // zero: the rows its state was built from are exactly the ones that have
    // gone, so this log no longer knows of a correction that touched them.
    // `ActionLog::forget_before` says which direction that fails a snapshot
    // ring in.
    assert_eq!(log.generation_at(HORIZON), 0);
}

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
