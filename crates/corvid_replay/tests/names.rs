//! The frozen *names*: every field and every variant of a session, in the order
//! `serde` was offered them. **Changing a value in this file is a wire-format
//! break.**
//!
//! `tests/wire.rs` freezes the bytes a capture is made of, and it carries no
//! names at all: the compact encoding writes fields in declaration order with no
//! label, so a struct and a tuple of the same fields are one byte string. Three
//! changes are therefore invisible there and visible here -- a field or a variant
//! renamed, two fields of the *same* type exchanged when the fixture holds the
//! same value in both, and a field added that encodes to no bytes. All three
//! change what a hand-written tool, a modding pipeline or a debugging dump reads
//! out of a capture.
//!
//! And one change is invisible in both: an integer widened. `4` is `4` here
//! whether it came from a `u8` or a `u64`, and a varint writes the same single
//! byte for either over there. What sees that one is the `Schema` digest an
//! `Opening` carries, which hashes the width a person wrote down. A change that
//! moves neither table and no digest has not moved anything a peer observes, and
//! that is the whole reason there is more than one.
//!
//! JSON is the self-describing format because it is already in the workspace's
//! dev-dependencies, and it is **not** a capture format -- nothing in Corvid is
//! ever written down in it. A row here is a claim about what a type looks like
//! to a reader that asks for names, and any format carrying names would make the
//! same claim differently.
//!
//! If a change here is genuinely wanted, it is a new version of the format: bump
//! the crate's major version, re-record every capture taken under the old one,
//! and say so in the changelog.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{golden_opening, golden_roster, golden_session, golden_trace, small_log};
use corvid_behavior::{PlayerId, ProfileId};
use corvid_replay::Seed;
use corvid_time::Tick;
use corvid_wire::golden::{Row, check_text};
use serde::{Deserialize, Serialize};

/// What `serde_json` writes for one value.
fn written<T: Serialize>(values: &[T]) -> Vec<String> {
    values
        .iter()
        .map(|value| serde_json::to_string(value).unwrap())
        .collect()
}

/// A seed is a bare number rather than a structure of any kind.
///
/// The row does *not* pin `#[serde(transparent)]`: a newtype struct writes its
/// inner value with or without the attribute, and deleting it moves nothing here
/// or anywhere else. What the row pins is that `Seed` stays a newtype over one
/// `u64` -- a second field would
/// make it an array and a named field would make it an object, and the byte
/// table would see neither of those as long as the same eight bytes came out.
/// The probe below is the demonstration.
const GOLDEN_SEED: &[Row<'_>] = &[("Seed", "72623859790382856")];

/// A profile names three fields, and `left` is `null` rather than absent -- a
/// `#[serde(skip_serializing_if)]` added to it would be invisible in the bytes
/// for the `None` case and visible here.
const GOLDEN_PROFILES: &[Row<'_>] = &[
    ("still playing", r#"{"account":17,"joined":4,"left":null}"#),
    (
        "joined at 5 and left at 6",
        r#"{"account":34,"joined":5,"left":6}"#,
    ),
];

/// A trace names its two fields, and the marks are numbers rather than objects.
const GOLDEN_TRACE: &[Row<'_>] = &[(
    "two marks from tick 4",
    r#"{"first":4,"marks":[1229801703532086340,6148933456521300104]}"#,
)];

/// A log names four fields, and its actions are named variants rather than
/// indices -- which is the half of an enum's encoding the byte table writes as a
/// number and cannot check.
const GOLDEN_LOG: &[Row<'_>] = &[(
    "two rows of two seats from tick 4",
    r#"{"first":4,"players":2,"actions":["Idle","Bump","Reset","Idle"],"confirmed":[6]}"#,
)];

/// The opening's eight fields. `content` is the level itself rather than a
/// handle, and `schema` is a bare number rather than an object, and both of
/// those are decisions a byte table records as anonymous bytes.
const GOLDEN_OPENING: &[Row<'_>] = &[(
    "terminus, two seats, opening at tick 4",
    r#"{"level":"terminus","content":{"name":"terminus","ceiling":7},"rules":{"step":2},"roster":[{"account":17,"joined":4,"left":null},{"account":34,"joined":5,"left":6}],"seed":72623859790382856,"first":4,"origin":{"count":5,"folded":6,"movers":[1],"roster":[9]},"schema":723685415333072913}"#,
)];

/// The whole session: three named fields, and everything above nested inside.
const GOLDEN_SESSION: &[Row<'_>] = &[(
    "the opening, the log and the trace",
    r#"{"opening":{"level":"terminus","content":{"name":"terminus","ceiling":7},"rules":{"step":2},"roster":[{"account":17,"joined":4,"left":null},{"account":34,"joined":5,"left":6}],"seed":72623859790382856,"first":4,"origin":{"count":5,"folded":6,"movers":[1],"roster":[9]},"schema":723685415333072913},"log":{"first":4,"players":2,"actions":["Idle","Bump","Reset","Idle"],"confirmed":[6]},"marks":{"first":4,"marks":[1229801703532086340,6148933456521300104]}}"#,
)];

#[test]
fn a_seed_is_written_as_a_bare_number() {
    check_text(
        "Seed",
        GOLDEN_SEED,
        &written(&[Seed(0x0102_0304_0506_0708)]),
    )
    .unwrap();
}

/// A newtype over the same `u64`, without `#[serde(transparent)]`.
#[derive(Serialize)]
struct Bare(u64);

/// The same one word behind a name.
#[derive(Serialize)]
struct Wrapped {
    bits: u64,
}

/// The same one word beside a second.
#[derive(Serialize)]
struct Widened(u64, u64);

#[test]
fn what_the_seed_row_would_and_would_not_catch() {
    // The attribute is not what makes the row what it is. `serde` writes a
    // newtype struct as its inner value on its own, so a `Seed` that dropped
    // `#[serde(transparent)]` writes the same text and this table stays put --
    // which is what the row's own comment says.
    let seed = serde_json::to_string(&Seed(7)).unwrap();
    assert_eq!(seed, serde_json::to_string(&Bare(7)).unwrap());

    // What it does catch is a `Seed` that stops being a newtype over one word.
    // Neither of these moves a byte the compact encoding writes -- a name costs
    // nothing there -- so this table is the only one that could.
    assert_ne!(seed, serde_json::to_string(&Wrapped { bits: 7 }).unwrap());
    assert_eq!(
        corvid_wire::encode(&Seed(7)).unwrap(),
        corvid_wire::encode(&Wrapped { bits: 7 }).unwrap(),
    );

    // And a second field is visible in both, which is the honest limit of the
    // claim: this row is not the only thing standing between a seed and a pair.
    assert_ne!(seed, serde_json::to_string(&Widened(7, 0)).unwrap());
    assert_ne!(
        corvid_wire::encode(&Seed(7)).unwrap(),
        corvid_wire::encode(&Widened(7, 0)).unwrap(),
    );
}

#[test]
fn a_profile_names_its_three_fields() {
    check_text("Profile", GOLDEN_PROFILES, &written(&golden_roster())).unwrap();
}

#[test]
fn a_trace_names_its_two() {
    check_text("HashTrace", GOLDEN_TRACE, &written(&[golden_trace()])).unwrap();
}

#[test]
fn a_log_names_its_four_and_its_actions_by_name() {
    check_text("ActionLog", GOLDEN_LOG, &written(&[small_log()])).unwrap();
}

#[test]
fn an_opening_names_its_eight() {
    check_text("Opening", GOLDEN_OPENING, &written(&[golden_opening()])).unwrap();
}

#[test]
fn a_session_names_its_three() {
    check_text("Session", GOLDEN_SESSION, &written(&[golden_session()])).unwrap();
}

// The three changes this table owns and the byte table cannot see, each written
// down under two declarations that differ by exactly one of them.

/// Two fields of the same type, as they are declared.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Named {
    joined: u64,
    left: u64,
}

/// The same two, exchanged.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Swapped {
    left: u64,
    joined: u64,
}

/// The same two under a different name for one of them.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Renamed {
    arrived: u64,
    left: u64,
}

/// And with a field that encodes to nothing added between them.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Marked {
    joined: u64,
    marker: (),
    left: u64,
}

#[test]
fn a_rename_is_a_change_only_this_table_sees() {
    let named = Named { joined: 4, left: 4 };
    let renamed = Renamed {
        arrived: 4,
        left: 4,
    };
    assert_eq!(
        corvid_wire::encode(&named).unwrap(),
        corvid_wire::encode(&renamed).unwrap(),
    );
    assert_ne!(
        serde_json::to_string(&named).unwrap(),
        serde_json::to_string(&renamed).unwrap(),
    );
}

#[test]
fn two_same_typed_fields_holding_one_value_swap_unseen_in_the_bytes() {
    // The qualification the byte table's claim about field order carries. Both
    // fields hold 4, so exchanging them writes the same sixteen bytes -- a
    // recorded byte row is the same row afterwards, and only the names move.
    let named = Named { joined: 4, left: 4 };
    let swapped = Swapped { left: 4, joined: 4 };
    assert_eq!(
        corvid_wire::encode(&named).unwrap(),
        corvid_wire::encode(&swapped).unwrap(),
    );
    assert_ne!(
        serde_json::to_string(&named).unwrap(),
        serde_json::to_string(&swapped).unwrap(),
    );
}

#[test]
fn a_field_that_writes_no_bytes_is_added_unseen_in_the_bytes() {
    let named = Named { joined: 4, left: 5 };
    let marked = Marked {
        joined: 4,
        marker: (),
        left: 5,
    };
    assert_eq!(
        corvid_wire::encode(&named).unwrap(),
        corvid_wire::encode(&marked).unwrap(),
    );
    assert_ne!(
        serde_json::to_string(&named).unwrap(),
        serde_json::to_string(&marked).unwrap(),
    );
}

#[test]
fn a_width_is_the_change_neither_table_sees() {
    // The other direction, so the pair is honest in both. A seat number at two
    // widths is one row here and one row there: JSON writes the number and the
    // bytes write the number, and neither writes the declaration.
    assert_eq!(
        serde_json::to_string(&PlayerId(2)).unwrap(),
        serde_json::to_string(&ProfileId(2)).unwrap(),
    );
    assert_eq!(
        corvid_wire::encode(&PlayerId(2)).unwrap(),
        corvid_wire::encode(&ProfileId(2)).unwrap(),
    );

    // The digest is the one that does, because `corvid_hash` absorbs an integer
    // as its declared bytes and injects the count of them at the end.
    assert_ne!(
        corvid_hash::digest(&PlayerId(2)),
        corvid_hash::digest(&ProfileId(2)),
    );
}

#[test]
fn a_tick_is_a_number_and_not_an_object() {
    // `Tick` is `#[serde(transparent)]` in `corvid_time`, and every tick in a
    // session goes through that. A change there would rewrite every row above.
    assert_eq!(serde_json::to_string(&Tick(4)).unwrap(), "4");
}
