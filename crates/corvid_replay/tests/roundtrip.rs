//! A session written down and read back, and the captures that are refused.
//!
//! The happy path and the checks a capture has to pass to become a session at
//! all: that it describes the same build, that its log and trace start where
//! the opening says, and that its roster is one a `PlayerId` can address.
//!
//! `tests/ragged.rs` holds the other half -- what happens to a capture that is
//! *accepted* while being the wrong shape inside.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use std::sync::Arc;

use common::{Counter, Level, forward, opening, play, schema};
use core::hash::Hash;
use corvid_behavior::{PlayerId, ProfileId};
use corvid_wire::round_trip_is_faithful;

use corvid_hash::{Digest, Hasher, digest};
use corvid_replay::{ActionLog, HashTrace, Load, Profile, Session, Shape, Snapshots};
use corvid_time::Tick;

/// Room for every state a two-hundred-tick session produces.
const ROOMY: usize = 1 << 24;

#[test]
fn a_session_replays_to_the_same_state_after_being_written_down() {
    let session = play(200);
    let (states, marks) = forward(&session);

    let bytes = session.save().unwrap();
    let read = Session::<Counter>::load(&bytes, schema()).unwrap();

    // The whole session, not just the state it happens to reach: the log, the
    // marks and the opening all have to come back, and `Eq` on a `Session`
    // compares all three.
    assert_eq!(read, session);

    let mut snapshots = Snapshots::new(ROOMY);
    let (state, _) = read.seek(&mut snapshots, read.last()).unwrap();
    assert_eq!(digest(&state), digest(&states[200]));
    assert_eq!(read.marks, marks);
}

#[test]
fn every_part_of_a_session_survives_the_workspaces_own_encoding() {
    // `corvid_behavior` owns the check and this is what it is for: the four
    // types a session is made of, each written out and read back through the
    // format a capture is written in.
    let session = play(20);
    round_trip_is_faithful(&session.opening.origin).unwrap();
    round_trip_is_faithful(&session.opening.rules).unwrap();
    round_trip_is_faithful(&*session.opening.content).unwrap();
    round_trip_is_faithful(session.log.get(Tick(3), PlayerId(0)).unwrap()).unwrap();
}

#[test]
fn a_capture_from_a_build_that_describes_itself_differently_is_refused() {
    // The refusal the schema exists for, with the error named. Without it this
    // capture loads under a build whose `State` means something else and the
    // divergence surfaces as a desync a hundred ticks later, on a peer.
    let session = play(20);
    let bytes = session.save().unwrap();
    let elsewhere = Digest::from_u64(0xdead_beef_dead_beef);

    let refused = Session::<Counter>::load(&bytes, elsewhere).unwrap_err();
    assert_eq!(
        refused,
        Load::Schema {
            recorded: schema(),
            running: elsewhere,
        },
    );
    // And it says which is which, because "the schemas differ" is not something
    // a person can act on.
    let said = refused.to_string();
    assert!(said.contains(&schema().to_string()), "{said}");
    assert!(said.contains(&elsewhere.to_string()), "{said}");

    // The same bytes under the build that wrote them load.
    assert!(Session::<Counter>::load(&bytes, schema()).is_ok());
}

#[test]
fn a_schema_moves_when_the_description_does() {
    // The other half of the refusal above: it is worth nothing unless two
    // descriptions of two builds actually differ. A field added to the
    // description moves the digest, so the capture stops loading.
    use corvid_replay::Schema;

    let before = Schema::new("counter").field("State.count", "i64").digest();
    let after = Schema::new("counter")
        .field("State.count", "i64")
        .field("State.folded", "u64")
        .digest();
    assert_ne!(before, after);

    // And a different game with the same fields is a different schema, so two
    // games in one process cannot load each other's captures.
    let other = Schema::new("bouncer").field("State.count", "i64").digest();
    assert_ne!(before, other);
}

#[test]
fn a_capture_whose_log_starts_elsewhere_is_refused() {
    let mut session = play(20);
    session.log = ActionLog::new(Tick(3), common::seats(&session));
    let bytes = session.save().unwrap();
    assert_eq!(
        Session::<Counter>::load(&bytes, schema()).unwrap_err(),
        Load::Shape(Shape::LogStart {
            log: Tick(3),
            opening: Tick::ZERO,
        }),
    );
}

#[test]
fn a_capture_whose_trace_starts_elsewhere_is_refused() {
    let mut session = play(20);
    session.marks = HashTrace::new(Tick(9));
    let bytes = session.save().unwrap();
    assert_eq!(
        Session::<Counter>::load(&bytes, schema()).unwrap_err(),
        Load::Shape(Shape::TraceStart {
            trace: Tick(9),
            opening: Tick::ZERO,
        }),
    );
}

#[test]
fn a_capture_whose_rows_are_not_as_wide_as_its_roster_is_refused() {
    // The one that would replay rather than fail: a row of three read against a
    // roster of four gives seat 3 `Action::default()` on every tick, which is a
    // session that never happened and looks exactly like a quiet player.
    let mut session = play(20);
    session.log = ActionLog::new(session.opening.first, 3);
    let bytes = session.save().unwrap();
    assert_eq!(
        Session::<Counter>::load(&bytes, schema()).unwrap_err(),
        Load::Shape(Shape::Width { log: 3, roster: 4 }),
    );

    // And with the roster narrowed to match, the same log loads -- so the check
    // is about the two agreeing rather than about the number three.
    let mut narrowed = play(20);
    narrowed.log = ActionLog::new(narrowed.opening.first, 3);
    narrowed.opening.roster.truncate(3);
    assert!(Session::<Counter>::load(&narrowed.save().unwrap(), schema()).is_ok());
}

#[test]
fn bytes_that_are_not_a_session_are_refused_with_the_encoders_reason() {
    assert!(matches!(
        Session::<Counter>::load(&[1, 2, 3], schema()),
        Err(Load::Bytes(_)),
    ));

    // And a capture that grew a field: the prefix still parses, and the
    // trailing bytes are what stop it loading as something it is not.
    let mut grown = play(4).save().unwrap();
    grown.push(0);
    assert!(matches!(
        Session::<Counter>::load(&grown, schema()),
        Err(Load::Bytes(corvid_wire::Error::Trailing { .. })),
    ));
}

#[test]
fn a_new_session_is_consistent_with_the_opening_it_was_built_from() {
    // What `Session::new` is for, and what makes every disagreement `check`
    // names a property of hand-assembly rather than of the ordinary path.
    let session = Session::new(opening()).unwrap();
    assert_eq!(session.log.first(), session.opening.first);
    assert_eq!(session.marks.first(), session.opening.first);
    assert_eq!(session.log.players(), common::seats(&session));
    assert_eq!(session.last(), session.opening.first);
    assert_eq!(session.check(), Ok(()));

    // And its one mark is the opening state *and the level*, so a peer
    // comparing traces from tick zero has something to compare, and a peer
    // holding a different build of the same file disagrees there rather than
    // once the contents start mattering.
    //
    // The **resolved** origin, not the `Option` field. An `Option`'s `Hash`
    // writes a discriminant before its payload, so an opening that stated its
    // origin and one that let it default would mark differently while opening
    // on the same state.
    let mut opener = Hasher::new();
    session.opening.origin().hash(&mut opener);
    session.opening.content.hash(&mut opener);
    assert_eq!(
        session.marks.get(session.opening.first),
        Some(opener.digest())
    );
}

/// The level is in the opening mark, which is what `Level` promises and what no
/// opt-out anywhere can switch off.
///
/// Two openings alike in every other way, differing only inside the level, mark
/// differently. Without this the guarantee is a sentence in a doc comment: the
/// assertion above would pass just as well against a mark taken of the origin
/// alone, because these tests never vary the level.
#[test]
fn the_level_is_in_the_opening_mark() {
    let session = Session::new(opening()).unwrap();

    let mut other = opening();
    other.content = Arc::new(Level {
        name: other.content.name.clone(),
        // The same level by name, built differently -- which is the case the
        // report is supposed to name a file for.
        ceiling: other.content.ceiling + 1,
    });
    let stale = Session::new(other).unwrap();

    assert_ne!(
        session.marks.get(session.opening.first),
        stale.marks.get(stale.opening.first),
        "two builds of one level opened to the same mark",
    );
}

#[test]
fn a_capture_carries_the_level_and_not_only_its_name() {
    // Why the opening carries the level itself and not only its name: a session
    // read back out of bytes seeks without anything else being handed to it.
    let session = play(30);
    let bytes = session.save().unwrap();
    let read = Session::<Counter>::load(&bytes, schema()).unwrap();
    assert_eq!(
        read.opening.content.ceiling,
        session.opening.content.ceiling
    );
    assert_eq!(read.opening.level, session.opening.level);

    let mut snapshots = Snapshots::new(ROOMY);
    assert!(read.seek(&mut snapshots, Tick(30)).is_ok());
}

#[test]
fn a_roster_read_back_reconstructs_the_same_presence() {
    let session = play(200);
    let read = Session::<Counter>::load(&session.save().unwrap(), schema()).unwrap();
    let expected: &[Profile] = &session.opening.roster;
    assert_eq!(read.opening.roster, expected);
    assert_eq!(
        read.opening.seat(PlayerId(2)).map(|seat| seat.account),
        Some(ProfileId(33)),
    );
    assert_eq!(read.opening.seat(PlayerId(9)), None);
}

#[test]
fn a_capture_with_more_seats_than_a_player_id_can_address_is_refused() {
    // A seat number is a `u16`, so a roster past sixty-five thousand has seats
    // no action can be attributed to -- `seek` would collapse every one of them
    // onto the last addressable seat, which is a session that never happened
    // rather than a decoding failure.
    let mut session = play(2);
    let seat = session.opening.roster[0];
    session.opening.roster = vec![seat; usize::from(u16::MAX) + 1];
    let bytes = session.save().unwrap();
    assert_eq!(
        Session::<Counter>::load(&bytes, schema()).unwrap_err(),
        Load::Shape(Shape::Roster {
            seats: usize::from(u16::MAX) + 1,
        }),
    );

    // And exactly `u16::MAX` seats is not refused for that reason, so the check
    // is about the boundary rather than about "a big roster".
    let mut widest = play(2);
    widest.opening.roster = vec![seat; usize::from(u16::MAX)];
    widest.log = ActionLog::new(widest.opening.first, u16::MAX);
    assert!(Session::<Counter>::load(&widest.save().unwrap(), schema()).is_ok());
}
