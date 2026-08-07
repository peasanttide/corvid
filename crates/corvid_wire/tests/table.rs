//! The golden helper: what it accepts, what it refuses, and what it says.
//!
//! A table helper is read once, when it is written, and then only ever seen
//! through its failures — which is when the person reading it is already
//! annoyed and is about to decide whether the red row is a real break or a test
//! to regenerate. So what it reports is as much of its job as what it checks,
//! and both are pinned here.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_wire::golden::{Row, check, hex, unhex};

const GOLDEN: &[Row<'_>] = &[("one", "01"), ("two", "02"), ("three", "03")];

const FIXTURE: &[u16] = &[1, 2, 3];

#[test]
fn a_table_that_still_holds_passes() {
    check("u16", GOLDEN, FIXTURE).unwrap();
}

#[test]
fn whitespace_in_a_row_means_nothing() {
    // Long rows are written in groups so a person can find the field they are
    // looking at. That has to cost nothing. These values each take a marker and
    // two bytes, so there is something in a row to group.
    let spaced: &[Row<'_>] = &[
        ("one", "fb 3412"),
        ("two", "fb\n7856"),
        ("three", "  fbbc9a  "),
    ];
    check("u16", spaced, &[0x1234_u16, 0x5678, 0x9abc]).unwrap();
}

#[test]
fn every_row_that_moved_is_reported_at_once() {
    // One row at a time is the obvious way to write a table check and the wrong
    // one: a deliberate format change moves every row and an accidental one
    // usually moves a handful, so the count and the shape of what moved are the
    // first two things worth knowing.
    let moved = check("u16", GOLDEN, &[9_u16, 2, 9]).unwrap_err();
    assert_eq!(moved.count(), 2);

    let report = moved.to_string();
    assert!(report.contains("2 of 3"), "{report}");
    assert!(report.contains("wire-format break"), "{report}");
}

#[test]
fn what_it_reports_is_what_the_table_is_written_as() {
    // The replacement is pasted, not transcribed. If the failure message is not
    // in the table's own syntax, somebody types it out and gets a digit wrong.
    let moved = check("u16", &GOLDEN[..1], &[9_u16]).unwrap_err();
    assert!(moved.to_string().contains("(\"one\", \"09\"),"), "{moved}");
}

#[test]
fn the_report_survives_being_unwrapped() {
    // A test says `unwrap`, and `unwrap` prints `Debug`. A derived `Debug` here
    // would put the report behind a wall of field names on the one occasion
    // anybody reads it.
    let moved = check("u16", &GOLDEN[..1], &[9_u16]).unwrap_err();
    assert_eq!(format!("{moved:?}"), moved.to_string());
}

#[test]
fn a_table_and_a_fixture_of_different_lengths_is_a_failure_and_not_a_silent_pass() {
    // The failure mode this catches is a row deleted along with its value, which
    // leaves a shorter table that agrees with a shorter fixture on every row it
    // still has.
    let short = check("u16", GOLDEN, &[1_u16, 2]).unwrap_err();
    assert!(short.to_string().contains("3 rows"), "{short}");
    assert!(short.to_string().contains("has 2"), "{short}");
}

#[test]
fn a_row_that_is_not_whole_bytes_says_so_rather_than_looking_like_a_break() {
    let malformed: &[Row<'_>] = &[("one", "010"), ("two", "02zz"), ("three", "03")];
    let moved = check("u16", malformed, FIXTURE).unwrap_err();
    assert_eq!(moved.count(), 2);
    assert!(moved.to_string().contains("not whole bytes"), "{moved}");
}

/// A type whose reader is not its writer's inverse.
///
/// `#[serde(skip)]` compiles, satisfies every bound in the workspace, and loses
/// a field. It is here because it is the one failure the *encoding* direction of
/// a table cannot see: the bytes are exactly what was recorded, and what comes
/// back out of them is not what went in.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Lossy {
    kept: u16,
    #[serde(skip)]
    dropped: u16,
}

#[test]
fn a_row_that_no_longer_reads_back_is_reported_even_though_it_still_encodes() {
    // The direction a round trip cannot supply, and the direction a save file
    // actually depends on. Both halves of this table's first check pass, because
    // the value that was recorded had nothing in the skipped field.
    let table: &[Row<'_>] = &[("kept 1", "01")];
    check(
        "Lossy",
        table,
        &[Lossy {
            kept: 1,
            dropped: 0,
        }],
    )
    .unwrap();

    // The same row against a value that does. It encodes to the recorded bytes,
    // so nothing has changed about what a capture holds — and reading that
    // capture gives a different value, which is the worst thing that can happen
    // to one: it loads, and it is wrong.
    let lost = check(
        "Lossy",
        table,
        &[Lossy {
            kept: 1,
            dropped: 9,
        }],
    )
    .unwrap_err();
    assert!(lost.to_string().contains("read back as"), "{lost}");
}

#[test]
fn hex_and_unhex_are_inverses() {
    let bytes: Vec<u8> = (0..=255).collect();
    assert_eq!(unhex(&hex(&bytes)).unwrap(), bytes);
    assert_eq!(hex(&[]), "");
    assert_eq!(unhex("").unwrap(), Vec::<u8>::new());

    // Lowercase is what `hex` writes and both cases are what it reads, because a
    // row pasted from somewhere else is not worth a failure that says nothing
    // about the format.
    assert_eq!(hex(&[0xde, 0xad]), "dead");
    assert_eq!(unhex("DEAD").unwrap(), vec![0xde, 0xad]);
    assert_eq!(unhex("de ad"), unhex("dead"));

    assert_eq!(unhex("f"), None);
    assert_eq!(unhex("gg"), None);
}
