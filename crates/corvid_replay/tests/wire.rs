//! The frozen serialized bytes of a session. **Changing a value in this file is
//! a wire-format break.**
//!
//! A capture *is* these bytes. A save file written by today's build is opened by
//! next year's, and a snapshot written by one peer is read by another on a
//! different commit, so what a `Session` encodes to is a published format and
//! not an implementation detail.
//!
//! Nothing symmetric can hold it still. `tests/roundtrip.rs` writes a session
//! out and reads it back with one build, so the writer and the reader move
//! together: exchange two fields of different types or renumber a variant, and
//! the round trip passes with the value it started from while every capture
//! recorded yesterday now means something else. Only a
//! literal nobody regenerated sees that, and `corvid_wire::golden::check`
//! compares these both ways round — that today's encoder writes them, and that
//! they still read back as the values they were recorded from.
//!
//! `tests/names.rs` is the other half, over the same fixtures. This table
//! carries no names at all, so it cannot see a field renamed, two same-typed
//! fields exchanged when the fixture holds the same value in both, or a field
//! added that encodes to nothing; that table sees exactly those. Neither
//! substitutes for the other, and neither sees an integer *widen* — a varint
//! spells a small number the same at every width and JSON spells it the same
//! again. What catches a widening is the `Schema` digest an `Opening` carries,
//! and `a_widened_field_moves_no_row_here_and_is_caught_by_the_schema` is where
//! that is written down.
//!
//! If a change here is genuinely wanted, it is a new version of the format: bump
//! the crate's major version, reissue every capture recorded under the old one,
//! and say so in the changelog. Regenerating these literals to make a red test
//! go green is never the right move — the red test *is* the notification that a
//! capture written by an older build has stopped loading as what it was.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{
    Action, Counter, golden_opening, golden_roster, golden_session, golden_trace, small_log,
};
use corvid_behavior::PlayerId;
use corvid_replay::{ActionLog, HashTrace, Opening, Profile, Schema, Seed, Session};
use corvid_time::Tick;
use corvid_wire::golden::{Row, check};
use serde::{Deserialize, Serialize};

/// A seed is its eight bytes, least significant first, and no tag.
const GOLDEN_SEED: &[Row<'_>] = &[("Seed(0x0102030405060708)", "fd0807060504030201")];

/// A profile: the account, the tick it joined on, and an `Option<Tick>` that is
/// one tag byte and then eight more if there is a tick.
const GOLDEN_PROFILES: &[Row<'_>] = &[
    ("still playing", "110400"),
    ("joined at 5 and left at 6", "22050106"),
];

/// A trace: the first tick, then a count and that many digests as raw `u64`.
const GOLDEN_TRACE: &[Row<'_>] = &[(
    "two marks from tick 4",
    "0402fd4444333322221111fd8888777766665555",
)];

/// A log: the first tick, the seat count, then the actions and then the
/// confirmation bits.
///
/// Two rows of two seats is four actions, each a variant index, and one byte of
/// bits — `06` is seats one and two of the four entries, which are the two this
/// fixture wrote.
const GOLDEN_LOG: &[Row<'_>] = &[(
    "two rows of two seats from tick 4",
    "04 02 04 00 01 02 00 01 06",
)];

/// The opening, field by field in declaration order.
const GOLDEN_OPENING: &[Row<'_>] = &[(
    "terminus, two seats, opening at tick 4",
    "08 7465726d696e7573 \
     08 7465726d696e7573 0e \
     04 \
     02 11 04 00 \
        22 05 01 06 \
     fd 0807060504030201 \
     04 \
     0a 06 01 01 01 09 \
     fd 11100f0e0d0c0b0a",
)];

/// The whole session: the opening, then the log, then the trace.
const GOLDEN_SESSION: &[Row<'_>] = &[(
    "the opening, the log and the trace",
    "08 7465726d696e7573 \
     08 7465726d696e7573 0e \
     04 \
     02 11 04 00 \
        22 05 01 06 \
     fd 0807060504030201 \
     04 \
     0a 06 01 01 01 09 \
     fd 11100f0e0d0c0b0a \
     04 02 04 00 01 02 00 01 06 \
     04 02 fd 4444333322221111 fd 8888777766665555",
)];

#[test]
fn a_seed_is_its_bits() {
    check("Seed", GOLDEN_SEED, &[Seed(0x0102_0304_0506_0708)]).unwrap();
}

#[test]
fn a_profile_writes_its_account_its_join_and_its_leave() {
    check("Profile", GOLDEN_PROFILES, &golden_roster()).unwrap();
}

#[test]
fn a_trace_writes_its_first_tick_and_then_its_marks() {
    check("HashTrace", GOLDEN_TRACE, &[golden_trace()]).unwrap();
}

#[test]
fn a_log_writes_its_actions_and_then_its_confirmation_bits() {
    check("ActionLog", GOLDEN_LOG, &[small_log()]).unwrap();
}

#[test]
fn an_opening_writes_its_fields_in_declaration_order() {
    check("Opening", GOLDEN_OPENING, &[golden_opening()]).unwrap();
}

#[test]
fn a_session_writes_its_opening_its_log_and_its_marks() {
    check("Session", GOLDEN_SESSION, &[golden_session()]).unwrap();
}

// The four changes this table owns, each written down under two declarations
// that differ by exactly one of them. Without these the table is a set of
// literals nobody has shown can move.

/// The order two fields of different types are declared in.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Declared {
    first: u64,
    players: u16,
}

/// The same two, exchanged. This compiles, and it is a different format.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Exchanged {
    players: u16,
    first: u64,
}

/// A seat number as it is declared.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Narrow {
    seat: u16,
}

/// The same field, widened. Also compiles, also a different format.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Widened {
    seat: u32,
}

/// An action as the fixture declares it.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Numbered {
    Idle,
    Bump,
    Reset,
}

/// The same three, renumbered by moving one.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Renumbered {
    Bump,
    Idle,
    Reset,
}

/// A log header, and the same header with one more field in it.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Grown {
    first: u64,
    players: u16,
    generation: u8,
}

#[test]
fn two_exchanged_fields_move_the_bytes() {
    let declared = corvid_wire::encode(&Declared {
        first: 4,
        players: 2,
    })
    .unwrap();
    let exchanged = corvid_wire::encode(&Exchanged {
        players: 2,
        first: 4,
    })
    .unwrap();
    assert_ne!(declared, exchanged);
    // Same length, so a table that only checked how long a row was would miss
    // it — which is the point of recording the bytes rather than the size.
    assert_eq!(declared.len(), exchanged.len());
}

#[test]
fn a_widened_field_moves_no_row_here_and_is_caught_by_the_schema() {
    // The one change none of the three tables above can see. A varint spells `2`
    // the same at every width, and so does a self-describing format; a digest
    // does see it, but a `Session` is compared by its recorded bytes here rather
    // than by its digest, so nothing in this file moves.
    let narrow = corvid_wire::encode(&Narrow { seat: 2 }).unwrap();
    let widened = corvid_wire::encode(&Widened { seat: 2 }).unwrap();
    assert_eq!(narrow, [0x02]);
    assert_eq!(widened, [0x02]);

    // What catches it is the digest an `Opening` carries. `Schema` hashes the
    // width a person wrote down beside each field, so two builds that describe
    // `u16` and `u32` disagree before a byte of the log is read — and a build
    // that widened the type without editing the description does not, which is
    // the limit `Schema` states in its own documentation.
    assert_ne!(
        Schema::new("counter").field("Seat.seat", "u16").digest(),
        Schema::new("counter").field("Seat.seat", "u32").digest(),
    );
}

#[test]
fn a_renumbered_variant_moves_the_bytes() {
    assert_ne!(
        corvid_wire::encode(&Numbered::Bump).unwrap(),
        corvid_wire::encode(&Renumbered::Bump).unwrap(),
    );
    // And the fixture's own action is the same shape: a `u32` index and nothing
    // else, so adding a variant at the end is safe and moving one is not.
    assert_eq!(
        corvid_wire::encode(&Action::Bump).unwrap(),
        corvid_wire::encode(&Numbered::Bump).unwrap(),
    );
}

#[test]
fn an_added_field_that_writes_bytes_moves_the_bytes() {
    let before = corvid_wire::encode(&Declared {
        first: 4,
        players: 2,
    })
    .unwrap();
    let after = corvid_wire::encode(&Grown {
        first: 4,
        players: 2,
        generation: 0,
    })
    .unwrap();
    assert_ne!(before, after);
    assert_eq!(after.len(), before.len() + 1);
}

#[test]
fn the_recorded_rows_still_read_back_as_a_playable_session() {
    // The direction a round trip cannot supply and a game actually depends on:
    // the bytes written down above, read by today's build, are a session that
    // still seeks. `check` compares the value; this compares what the value
    // does.
    let bytes = corvid_wire::golden::unhex(GOLDEN_SESSION[0].1).unwrap();
    let session: Session<Counter> = corvid_wire::decode(&bytes).unwrap();
    assert_eq!(session.first(), Tick(4));
    assert_eq!(session.last(), Tick(6));

    let mut snapshots = corvid_replay::Snapshots::new(1 << 16);
    let (state, _) = session.seek(&mut snapshots, Tick(6)).unwrap();
    assert_eq!(state.count, 0);
    assert_eq!(state.movers, [PlayerId(0)]);
}

#[test]
fn the_fixtures_are_the_ones_the_name_table_records() {
    // The two tables are two views of one set of values rather than two suites,
    // and this is the line that keeps them so: every fixture named in
    // `tests/names.rs` is built here from the same functions.
    let session = golden_session();
    assert_eq!(session.opening, golden_opening());
    assert_eq!(session.log, small_log());
    assert_eq!(session.marks, golden_trace());
    assert_eq!(session.opening.roster, golden_roster());
    let _: ActionLog<Action> = small_log();
    let _: HashTrace = golden_trace();
    let _: Opening<Counter> = golden_opening();
    let _: Vec<Profile> = golden_roster();
}
