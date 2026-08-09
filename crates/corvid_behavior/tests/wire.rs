//! The frozen serialized forms. **Changing a value in this file is a
//! wire-format break.**
//!
//! Two tables live here and a third lives in `tests/golden.rs`, and each is
//! blind where the others see. The byte table is what `corvid_wire` writes,
//! which carries no names at all; the text table below is what a
//! *self-describing* format writes over the same fixtures, which carries every
//! name and no width; and `tests/golden.rs` freezes what the derived [`Hash`]
//! implementations produce under `corvid_hash`'s hasher.
//!
//! So: a field *renamed* moves the text table and neither of the other two,
//! because a hasher absorbs values and never names and this crate's encoding
//! writes none. A field reordered and a variant moved move all three. An
//! identifier *widened* moves only the digest -- a varint carries a small number
//! the same at every width, and a self-describing format writes the same number
//! too. None of the four is a compile error.
//!
//! Nor can a round trip see any of them. `round_trip_is_faithful` and the tests
//! in `tests/contract.rs` write a value out and read it back with one build, so
//! the writer and the reader move together: reorder two fields and the bytes on
//! disk change while the round trip still passes with the value it started
//! from. What breaks is the snapshot a peer on yesterday's build sends, and
//! nothing symmetric is holding those bytes still.
//!
//! So the bytes are written down here, over the fixtures in
//! `tests/common/vocabulary.rs` -- the same values `tests/golden.rs` digests, so
//! that the three tables are three views of one set of values rather than three
//! suites. `corvid_wire::golden::check` compares the bytes both ways round: that
//! today's encoder writes these bytes, and that these bytes still read back as
//! the values they were recorded from. `check_text` compares the text one way
//! round, which is all there is to compare -- what a text table is for is the
//! names, and a name that changed changed in the writing.
//!
//! If a change here is genuinely wanted, it is a new version of the format:
//! bump the crate's major version, reissue every capture recorded under the old
//! one, and say so in the changelog. Regenerating these literals to make a red
//! test go green is never the right move -- the red test *is* the notification
//! that a snapshot written by an older build has stopped loading as what it was.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::vocabulary::every_presence;
use corvid_behavior::{
    AchievementId, ExitCode, LobbyId, PlayerId, PresenceText, ProfileId, RumbleId, SaveSlot,
    StatId, Url,
};
use corvid_hash::digest;
use corvid_wire::golden::{Row, check, check_text};

/// Every [`Presence`](corvid_behavior::Presence), including the one with no
/// payload -- which is one byte of index and nothing else, and is what pins that
/// a payload-free variant still writes its number.
const GOLDEN_PRESENCE: &[Row<'_>] = &[
    ("Joining(77)", "004d"),
    ("Active", "01"),
    ("Dropped(4)", "0204"),
];

/// The identifiers, which are transparent newtypes: an identifier is its
/// integer and nothing else.
///
/// Three widths are represented and none of them is visible as a length. An
/// [`ExitCode`] holding 2, a [`PlayerId`] holding 2 and a [`ProfileId`] holding
/// 2 are all the single byte `02`, because a varint spells a small number the
/// same however it was declared -- so what this table pins is the *value* each
/// identifier writes and the marker a large one takes, not its width.
/// `PlayerId(u16::MAX)` and `LobbyId(u64::MAX)` are the rows where the marker
/// shows: `fb` says two bytes follow and `fd` says eight.
///
/// Widening one of these moves nothing here. It moves `tests/golden.rs`, where
/// the digest is, because the hasher injects the count of bytes it absorbed and
/// a wider integer absorbs more of them --
/// [`an_identifiers_width_is_in_the_digest_and_not_in_the_bytes`] is that pair
/// of facts without the table.
///
/// [`an_identifiers_width_is_in_the_digest_and_not_in_the_bytes`]:
///     an_identifiers_width_is_in_the_digest_and_not_in_the_bytes
const GOLDEN_IDS: &[Row<'_>] = &[
    ("ExitCode::SUCCESS", "00"),
    ("ExitCode::FAILURE", "01"),
    ("ExitCode(255)", "ff"),
    ("PlayerId(0)", "00"),
    ("PlayerId(2)", "02"),
    ("PlayerId(u16::MAX)", "fbffff"),
    ("SaveSlot(2)", "02"),
    ("RumbleId(2)", "02"),
    ("AchievementId(2)", "02"),
    ("StatId(2)", "02"),
    ("ProfileId(77)", "4d"),
    ("LobbyId(u64::MAX)", "fdffffffffffffffff"),
];

/// The names, which are written as the strings they are rather than as the
/// NUL-padded arrays they are stored in.
///
/// The empty row is the one that says so: a name written as its storage would
/// be sixteen bytes of zero here and two hundred and fifty-six for a [`Url`],
/// and every name would cost its capacity in every capture.
const GOLDEN_NAMES: &[Row<'_>] = &[
    ("PresenceText::EMPTY", "00"),
    ("PresenceText(cradle)", "06637261646c65"),
    (
        "Url(https://example.invalid)",
        "1768747470733a2f2f6578616d706c652e696e76616c6964",
    ),
];

/// Every presence, by name -- including `Active`, whose byte row is one index
/// and nothing else and whose name appears nowhere in it.
const GOLDEN_PRESENCE_TEXT: &[Row<'_>] = &[
    ("Joining(77)", r#"{"Joining":{"profile":77}}"#),
    ("Active", r#""Active""#),
    ("Dropped(4)", r#"{"Dropped":{"since":4}}"#),
];

#[test]
fn every_presence_serializes_to_its_recorded_bytes() {
    check("Presence", GOLDEN_PRESENCE, &every_presence()).unwrap();
}

#[test]
fn the_identifiers_serialize_to_their_recorded_bytes() {
    check(
        "ExitCode",
        &GOLDEN_IDS[..3],
        &[ExitCode::SUCCESS, ExitCode::FAILURE, ExitCode(255)],
    )
    .unwrap();
    check(
        "PlayerId",
        &GOLDEN_IDS[3..6],
        &[PlayerId(0), PlayerId(2), PlayerId(u16::MAX)],
    )
    .unwrap();
    check("SaveSlot", &GOLDEN_IDS[6..7], &[SaveSlot(2)]).unwrap();
    check("RumbleId", &GOLDEN_IDS[7..8], &[RumbleId(2)]).unwrap();
    check("AchievementId", &GOLDEN_IDS[8..9], &[AchievementId(2)]).unwrap();
    check("StatId", &GOLDEN_IDS[9..10], &[StatId(2)]).unwrap();
    check("ProfileId", &GOLDEN_IDS[10..11], &[ProfileId(77)]).unwrap();
    check("LobbyId", &GOLDEN_IDS[11..], &[LobbyId(u64::MAX)]).unwrap();
}

#[test]
fn the_names_serialize_as_the_strings_they_are() {
    check(
        "PresenceText",
        &GOLDEN_NAMES[..2],
        &[PresenceText::EMPTY, PresenceText::new("cradle").unwrap()],
    )
    .unwrap();
    check(
        "Url",
        &GOLDEN_NAMES[2..],
        &[Url::new("https://example.invalid").unwrap()],
    )
    .unwrap();
}

#[test]
fn an_identifiers_width_is_in_the_digest_and_not_in_the_bytes() {
    // The claim the identifier table rests on, said once without the table.
    // Three identifiers holding the same number, declared at one, two and eight
    // bytes, write the *same* single byte: a varint carries the value and not
    // the width, so no recorded row in this file moves when one of them is
    // widened.
    assert_eq!(corvid_wire::encode(&ExitCode(2)).unwrap(), [0x02]);
    assert_eq!(corvid_wire::encode(&PlayerId(2)).unwrap(), [0x02]);
    assert_eq!(corvid_wire::encode(&ProfileId(2)).unwrap(), [0x02]);

    // The digest is where the width is. `corvid_hash` absorbs an integer as its
    // declared bytes and injects the total count at the end, so these three
    // differ -- and a peer comparing digests, which is what a peer actually
    // compares, is what refuses a build that widened one of them.
    assert_ne!(digest(&ExitCode(2)), digest(&PlayerId(2)));
    assert_ne!(digest(&ExitCode(2)), digest(&ProfileId(2)));
    assert_ne!(digest(&PlayerId(2)), digest(&ProfileId(2)));
}

/// Every fixture, written by a self-describing format, for [`check_text`].
fn as_text<T: serde::Serialize>(values: &[T]) -> Vec<String> {
    values
        .iter()
        .map(|value| serde_json::to_string(value).unwrap())
        .collect()
}

#[test]
fn every_presence_writes_its_recorded_names() {
    check_text(
        "Presence",
        GOLDEN_PRESENCE_TEXT,
        &as_text(&every_presence()),
    )
    .unwrap();
}
