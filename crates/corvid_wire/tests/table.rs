//! The golden helper: what it accepts, what it refuses, and what it says.
//!
//! A table helper is read once, when it is written, and then only ever seen
//! through its failures -- which is when the person reading it is already
//! annoyed and is about to decide whether the red row is a real break or a test
//! to regenerate. So what it reports is as much of its job as what it checks,
//! and both are pinned here.
//!
//! The digest and text helpers are checked here too. Neither compares a format
//! this crate defines, but both are the same report reached by a different
//! route, and a crate that keeps all three tables meets all three failures.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_wire::golden::{DigestRow, Row, check, check_digests, check_text, grouped, hex, unhex};

const GOLDEN: &[Row<'_>] = &[("one", "01"), ("two", "02"), ("three", "03")];

const FIXTURE: &[u16] = &[1, 2, 3];

/// A digest table, in the form a crate that keeps one writes it.
const GOLDEN_DIGESTS: &[DigestRow<'_>] = &[
    ("the opening tick", 0x7383_3581_a38e_f3cd),
    ("the tick after it", 0x3178_2188_0dd5_d02b),
];

/// A self-describing table, in the form a crate that keeps one writes it.
const GOLDEN_TEXT: &[Row<'_>] = &[
    ("the origin", r#"{"x":0,"y":0}"#),
    ("one step across", r#"{"x":1,"y":0}"#),
];

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
    // so nothing has changed about what a capture holds -- and reading that
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

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Inner {
    a: u32,
    b: u32,
}

/// A type this format cannot write at all.
///
/// `#[serde(flatten)]` asks for a map whose length is not known until its
/// contents are, and a format that writes a count first cannot begin one. What
/// it is doing in a file about the *helper* is that it is the only way a row
/// fails before there are any bytes to compare -- `tests/named.rs` is where the
/// refusal itself is pinned.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Flattened {
    tick: u32,
    #[serde(flatten)]
    inner: Inner,
}

/// A type this format writes and cannot read.
///
/// An untagged enum's writer emits the chosen variant's payload and nothing
/// else, and its reader then asks the bytes which variant that was. It is here
/// because it is the one shape whose recorded row is perfectly good bytes that
/// no longer come back -- the failure that only the second direction of a check
/// can see, arriving on its own rather than alongside a moved row.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum Untagged {
    One(u32),
    Two { a: u32, b: u32 },
}

#[test]
fn a_value_that_will_not_encode_is_said_to_have_been_refused() {
    // Reporting this as a moved row would offer a replacement literal for bytes
    // that do not exist, and somebody would paste it. The row is not wrong; the
    // type has stopped being writable, and those need different words.
    let table: &[Row<'_>] = &[("a flattened tick", "010203")];
    let refused = check(
        "Flattened",
        table,
        &[Flattened {
            tick: 1,
            inner: Inner { a: 2, b: 3 },
        }],
    )
    .unwrap_err();

    assert_eq!(refused.count(), 1);
    let report = refused.to_string();
    assert!(report.contains("would not encode"), "{report}");
    assert!(report.contains("a flattened tick"), "{report}");
    assert!(report.contains("SequenceMustHaveLength"), "{report}");
}

#[test]
fn a_recorded_row_that_no_longer_decodes_says_which_half_failed() {
    // The encoding half of this row passes: the value writes exactly the bytes
    // that were recorded. Only the reading half fails, so the report has to name
    // that half -- a capture full of these loads as nothing at all, and the reason
    // is not in its bytes.
    let table: &[Row<'_>] = &[("the two-field variant", "0709")];
    let stuck = check("Untagged", table, &[Untagged::Two { a: 7, b: 9 }]).unwrap_err();

    assert_eq!(stuck.count(), 1);
    let report = stuck.to_string();
    assert!(report.contains("no longer read back"), "{report}");
    assert!(report.contains("the two-field variant"), "{report}");
    assert!(report.contains("AnyNotSupported"), "{report}");

    // And the encoding half really did agree, so the finding above is about the
    // decoder and not about a row that had drifted anyway.
    assert_eq!(corvid_wire::encode(&Untagged::One(7)).unwrap(), [0x07]);
}

#[test]
fn a_digest_table_and_its_fixture_must_be_the_same_length() {
    // Same failure as the byte table's, and worth its own line because a digest
    // fixture is usually built by a loop rather than written out, so a table and
    // a fixture drift apart here more easily than anywhere else.
    let short = check_digests("Trace", GOLDEN_DIGESTS, &[0x7383_3581_a38e_f3cd]).unwrap_err();
    assert_eq!(short.count(), 1);
    assert!(short.to_string().contains("2 rows"), "{short}");
    assert!(short.to_string().contains("has 1"), "{short}");
}

#[test]
fn a_digest_that_moved_is_reported_as_a_row_to_paste() {
    // A digest is sixteen digits with no structure to read, so a report that
    // said only "row two moved" would send somebody to print the value by hand.
    let moved = check_digests("Trace", GOLDEN_DIGESTS, &[0x7383_3581_a38e_f3cd, 9]).unwrap_err();
    assert_eq!(moved.count(), 1);

    let report = moved.to_string();
    assert!(report.contains("1 of 2"), "{report}");
    assert!(
        report.contains(r#"("the tick after it", 0x0000_0000_0000_0009),"#),
        "{report}",
    );
}

#[test]
fn a_text_table_and_its_fixture_must_be_the_same_length() {
    let written = vec![r#"{"x":0,"y":0}"#.to_owned()];
    let short = check_text("Point", GOLDEN_TEXT, &written).unwrap_err();
    assert_eq!(short.count(), 1);
    assert!(short.to_string().contains("2 rows"), "{short}");
    assert!(short.to_string().contains("has 1"), "{short}");
}

#[test]
fn a_renamed_field_moves_the_text_row_it_appears_in() {
    // The change this table exists for: a rename is invisible to every byte row
    // in the workspace, so the only thing that can report it is the text a
    // self-describing format wrote.
    let written = vec![
        r#"{"x":0,"y":0}"#.to_owned(),
        r#"{"across":1,"y":0}"#.to_owned(),
    ];
    let moved = check_text("Point", GOLDEN_TEXT, &written).unwrap_err();
    assert_eq!(moved.count(), 1);

    let report = moved.to_string();
    assert!(report.contains("1 of 2"), "{report}");
    assert!(report.contains("one step across"), "{report}");
    assert!(report.contains(r#"{"across":1,"y":0}"#), "{report}");
}

#[test]
fn a_digest_is_written_at_its_full_width_even_when_it_is_mostly_zeros() {
    // A digest table is read down its column, and a short `0x9` beside a row of
    // sixteen digits is not a column. Both ends, because a truncating writer and
    // a wrapping one fail at opposite ones.
    assert_eq!(grouped(0), "0x0000_0000_0000_0000");
    assert_eq!(grouped(u64::MAX), "0xffff_ffff_ffff_ffff");

    // And a value with every digit distinct, so the groups are in the order they
    // are read in rather than reversed a group at a time.
    assert_eq!(grouped(0x1234_5678_9abc_def0), "0x1234_5678_9abc_def0");
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

#[test]
fn a_text_row_holding_a_quote_and_a_hash_is_still_a_literal_that_compiles() {
    // A raw string is terminated by a quote and as many hashes as opened it, so
    // a recorded row that itself contains `"#` closes the report's literal early
    // and the paste does not compile. JSON is exactly where that turns up: any
    // string field holding a `#` right after a quote is enough.
    let awkward = vec![r##"{"s":"\"#"}"##.to_string()];
    let moved = check_text("Awkward", &[("a name", "{}")], &awkward).unwrap_err();
    let report = moved.to_string();

    // Two hashes rather than one, because one would have been closed by the
    // `"#` in the middle of the row.
    assert!(report.contains(r###"r##"{"s":"\"#"}"##"###), "{report}");

    // And the ordinary row still takes the one hash the tables are written with,
    // so the fix costs nothing everywhere it was not needed.
    let plain = vec![r#"{"x":1,"y":0}"#.to_string()];
    let moved = check_text("Point", &GOLDEN_TEXT[..1], &plain).unwrap_err();
    assert!(
        moved.to_string().contains(r##"r#"{"x":1,"y":0}"#"##),
        "{moved}"
    );
}
