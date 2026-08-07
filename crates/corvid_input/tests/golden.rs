//! The frozen numbering and the frozen encodings. **Changing a value in this
//! file is a wire-format break.**
//!
//! There are two wire formats in this crate and they are not the same kind of
//! thing.
//!
//! The first is the numbering. `action_sets!` hands out identifiers from
//! declaration order, a binding file records that this player's `X` button is
//! `DigitalId(4)`, and that file outlives the build that wrote it. Move a
//! declaration and the file now points at somebody else's action — with no
//! compile error, no failed parse, and nothing to notice it but a player whose
//! controller stopped doing what they set it to. So the numbering for a fixed
//! declaration is written down here as literals, over a declaration shaped like
//! a game's rather than a fixture invented for the test, so that the table below
//! is the same shape a real binding file is read against.
//!
//! The second is the encoding of the parts a binding file is built out of. An
//! identifier writes itself down as a bare number and the two value types write
//! their fields in declaration order, so swapping two fields of the same type
//! is a change no comparison between values can see — and, in particular, one
//! that a serialize-then-deserialize test cannot see either. A round trip is
//! symmetric: the writer and the reader move together, so a reordered field, a
//! widened integer and a renumbered variant all still come back as the value
//! that went in, having been written down as different bytes on the way. There
//! is such a round trip at the bottom of this file and it is worth having, but
//! it proves that the encoding is self-consistent and not that it is the same
//! encoding as yesterday's. Only a literal proves that.
//!
//! So the encoding is frozen three times, because no single table sees all of
//! it. The JSON table freezes what a self-describing format writes down: the
//! field names, and the order they come in. The byte table freezes what the
//! workspace's own encoding writes down, which is `corvid_wire` — little-endian
//! variable-length integers, so a row is each value and no name at all. JSON
//! cannot show a field's *position*, and these bytes cannot show its name. A
//! change that moves neither table has not moved either of those.
//!
//! Neither shows an identifier's **width**: `4` is `4` in JSON whether it came
//! from a `u16` or a `u64`, and it is the single byte `04` here for the same
//! reason, because a varint picks its marker from the number rather than from
//! the type. The third table, at the bottom of this file, is the digest one —
//! `corvid_hash` absorbs an integer as its declared bytes and injects the count,
//! so a widening moves it and moves nothing else in this crate.
//!
//! The byte table is `corvid_wire`'s, which is the same encoding every other
//! capture in the workspace is recorded in. A binding file has no format of its
//! own and this crate serializes nothing itself.
//!
//! No table here is a test to regenerate. If a change is genuinely wanted it is
//! a new version of the format: bump the crate's major version, and give
//! bindings saved under the old numbering somewhere to go. Making a red row
//! green by pasting the new value in is how a player loses their bindings
//! silently.
//!
//! Nothing in this crate is an enum, so there are no discriminants of its own to
//! freeze — the identifiers are the discriminants, and they are the first table.
//! The one enum on its wire is the `Option` around a pointer that is not there,
//! and the byte table writes down which variant that is.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

// Only the encoding tables name these types. The numbering tests read the
// identifiers through the constants the macro generated and never have to say
// what they are, so without `serde` this import is unused and warns.
#[cfg(feature = "serde")]
use corvid_input::{AnalogId, DigitalId, PoseId, SetId};
#[cfg(feature = "serde")]
use corvid_wire::golden::Row;

/// The declaration every table below is frozen against: a menu, a build mode
/// and a swarm mode, with all three kinds of action between them.
pub mod action {
    corvid_input::action_sets! {
        pub set Menu {
            digital NAVIGATE_UP, NAVIGATE_DOWN, ACTIVATE, BACK;
        }
        pub set Build {
            digital PLACE, CANCEL, ROTATE_CW, ROTATE_CCW;
            analog LOOK, MOVE;
            pose POINTER;
        }
        pub set Swarm {
            digital DROP, ABILITY_EMP, ABILITY_BURROW;
            analog SPIN, ZOOM;
            pose GRAB_LEFT, GRAB_RIGHT;
        }
    }
}

/// One row of the set table: name, number, and the first and count of each of
/// the three kinds.
type SetRow = (&'static str, u16, u16, u16, u16, u16, u16, u16);

/// Every set, as `(name, id, first digital, digitals, first analog, analogs,
/// first pose, poses)`.
///
/// The three `first` columns are what a swap of two declarations moves, and
/// they are three columns rather than one because the three kinds are numbered
/// in separate spaces: an implementation that ran one counter across all three
/// would put `Build`'s analog actions at four, and only this table would say
/// so.
const GOLDEN_SETS: &[SetRow] = &[
    ("Menu", 0, 0, 4, 0, 0, 0, 0),
    ("Build", 1, 4, 4, 0, 2, 0, 1),
    ("Swarm", 2, 8, 3, 2, 2, 1, 2),
];

/// Every digital action and the number it was given.
const GOLDEN_DIGITAL: &[(&str, u16)] = &[
    ("NAVIGATE_UP", 0),
    ("NAVIGATE_DOWN", 1),
    ("ACTIVATE", 2),
    ("BACK", 3),
    ("PLACE", 4),
    ("CANCEL", 5),
    ("ROTATE_CW", 6),
    ("ROTATE_CCW", 7),
    ("DROP", 8),
    ("ABILITY_EMP", 9),
    ("ABILITY_BURROW", 10),
];

/// Every analog action. `Menu` declares none, so `Build`'s start at zero while
/// its digital actions start at four — the row that pins the spaces apart.
const GOLDEN_ANALOG: &[(&str, u16)] = &[("LOOK", 0), ("MOVE", 1), ("SPIN", 2), ("ZOOM", 3)];

/// Every pose.
const GOLDEN_POSE: &[(&str, u16)] = &[("POINTER", 0), ("GRAB_LEFT", 1), ("GRAB_RIGHT", 2)];

#[test]
fn the_table_is_what_it_was_recorded_as() {
    let found: Vec<SetRow> = action::SETS
        .iter()
        .map(|set| {
            (
                set.name(),
                set.id().0,
                set.digital().first(),
                set.digital().count(),
                set.analog().first(),
                set.analog().count(),
                set.pose().first(),
                set.pose().count(),
            )
        })
        .collect();

    assert_eq!(
        found, GOLDEN_SETS,
        "the numbering moved, which is a wire-format break and not a test to \
         regenerate: every binding file saved under the recorded numbering now \
         names a different action",
    );
}

#[test]
fn every_digital_action_is_numbered_as_it_was_recorded() {
    let found = [
        ("NAVIGATE_UP", action::NAVIGATE_UP),
        ("NAVIGATE_DOWN", action::NAVIGATE_DOWN),
        ("ACTIVATE", action::ACTIVATE),
        ("BACK", action::BACK),
        ("PLACE", action::PLACE),
        ("CANCEL", action::CANCEL),
        ("ROTATE_CW", action::ROTATE_CW),
        ("ROTATE_CCW", action::ROTATE_CCW),
        ("DROP", action::DROP),
        ("ABILITY_EMP", action::ABILITY_EMP),
        ("ABILITY_BURROW", action::ABILITY_BURROW),
    ];
    check(GOLDEN_DIGITAL, &found.map(|(name, id)| (name, id.0)));
}

#[test]
fn every_analog_action_is_numbered_as_it_was_recorded() {
    let found = [
        ("LOOK", action::LOOK),
        ("MOVE", action::MOVE),
        ("SPIN", action::SPIN),
        ("ZOOM", action::ZOOM),
    ];
    check(GOLDEN_ANALOG, &found.map(|(name, id)| (name, id.0)));
}

#[test]
fn every_pose_is_numbered_as_it_was_recorded() {
    let found = [
        ("POINTER", action::POINTER),
        ("GRAB_LEFT", action::GRAB_LEFT),
        ("GRAB_RIGHT", action::GRAB_RIGHT),
    ];
    check(GOLDEN_POSE, &found.map(|(name, id)| (name, id.0)));
}

/// The parts of a binding file, as JSON.
///
/// Every value here is asymmetric on purpose. `DigitalId(4)` and `SetId(2)` are
/// different numbers, so an identifier that wrapped itself in an object or that
/// wrote a type tag alongside its number moves a row; `Digital` has one field
/// true and two false with the true one in the middle, so swapping any two of
/// the three moves a row; and `Analog`'s two axes are different numbers of
/// different signs, so swapping them moves a row too. A table of zeroes and
/// identical fields would pass under every one of those.
#[cfg(feature = "serde")]
const GOLDEN_JSON: &[Row<'_>] = &[
    ("DigitalId(4)", "4"),
    ("AnalogId(1)", "1"),
    ("PoseId(2)", "2"),
    ("SetId(2)", "2"),
    (
        "Digital pressed but not held",
        r#"{"held":false,"pressed":true,"released":false}"#,
    ),
    (
        "Digital held",
        r#"{"held":true,"pressed":false,"released":false}"#,
    ),
    ("Analog(30000, -4000)", r#"{"x":30000,"y":-4000}"#),
    ("Analog::ZERO", r#"{"x":0,"y":0}"#),
];

#[cfg(feature = "serde")]
#[test]
fn the_parts_of_a_binding_file_encode_as_they_were_recorded() {
    use corvid_fixed::Signed16;
    use corvid_input::{Analog, Digital};
    let written = [
        serde_json::to_string(&DigitalId(4)),
        serde_json::to_string(&AnalogId(1)),
        serde_json::to_string(&PoseId(2)),
        serde_json::to_string(&SetId(2)),
        serde_json::to_string(&Digital {
            held: false,
            pressed: true,
            released: false,
        }),
        serde_json::to_string(&Digital::HELD),
        serde_json::to_string(&Analog::new(
            Signed16::from_bits(30_000),
            Signed16::from_bits(-4_000),
        )),
        serde_json::to_string(&Analog::ZERO),
    ]
    .into_iter()
    .map(|json| json.expect("these values all encode"))
    .collect::<Vec<String>>();

    corvid_wire::golden::check_text("a binding file's parts", GOLDEN_JSON, &written).unwrap();
}

/// The same parts, as bytes.
///
/// The table above is JSON and this one is not, because the two see different
/// halves of the same encoding. JSON writes the field names down, so it is what
/// catches a field renamed; these rows carry no names, so they are what catches
/// a field reordered and a value changed.
///
/// Neither sees an identifier's width. `4` is `4` in JSON whether the identifier
/// behind it is a `u16` or a `u64`, and it is the single byte `04` here for the
/// same reason — a varint spells the value and not the declaration. The digest
/// table at the bottom of this file is the one that moves on a widening, because
/// the hasher absorbs an integer as its declared bytes and injects the count.
///
/// The values are the ones the JSON table uses, row for row, so the two can be
/// read side by side, with two more on the end for the pointer.
#[cfg(feature = "serde")]
const GOLDEN_BYTES: &[Row<'_>] = &[
    ("DigitalId(4)", "04"),
    ("AnalogId(1)", "01"),
    ("PoseId(2)", "02"),
    ("SetId(2)", "02"),
    ("Digital pressed but not held", "000100"),
    ("Digital held", "010000"),
    ("Analog(30000, -4000)", "fb60ea fb3f1f"),
    ("Analog::ZERO", "0000"),
    ("no pointer", "00"),
    ("a pointer at (30000, -4000)", "01 fb60ea fb3f1f"),
];

#[cfg(feature = "serde")]
#[test]
fn the_parts_of_a_binding_file_are_the_bytes_they_were_recorded_as() {
    use corvid_fixed::Signed16;
    use corvid_input::{Analog, Digital};
    use corvid_wire::golden::check;

    let pushed = Analog::new(Signed16::from_bits(30_000), Signed16::from_bits(-4_000));
    // One call per type rather than one over four numbers: what is being frozen
    // is each identifier's own encoding, and a newtype that stopped being
    // transparent — wrapping its number in a struct, or writing a tag beside it
    // — would be invisible to a table that had already unwrapped it.
    check("DigitalId", &GOLDEN_BYTES[..1], &[DigitalId(4)]).unwrap();
    check("AnalogId", &GOLDEN_BYTES[1..2], &[AnalogId(1)]).unwrap();
    check("PoseId", &GOLDEN_BYTES[2..3], &[PoseId(2)]).unwrap();
    check("SetId", &GOLDEN_BYTES[3..4], &[SetId(2)]).unwrap();
    check(
        "Digital",
        &GOLDEN_BYTES[4..6],
        &[
            Digital {
                held: false,
                pressed: true,
                released: false,
            },
            Digital::HELD,
        ],
    )
    .unwrap();
    check("Analog", &GOLDEN_BYTES[6..8], &[pushed, Analog::ZERO]).unwrap();

    // The pointer, which is the only enum on this crate's wire: an `Option` is
    // a tag byte and then its payload, and the two rows are what pins which tag
    // is which. Nothing else here is an enum — the identifiers are the
    // discriminants, and they are the first table in this file.
    check("Option<Analog>", &GOLDEN_BYTES[8..], &[None, Some(pushed)]).unwrap();
}

#[cfg(feature = "serde")]
#[test]
fn what_a_binding_file_writes_down_is_what_it_reads_back() {
    use corvid_fixed::Signed16;
    use corvid_input::{Analog, Digital};
    // The golden above pins what the bytes are; this pins that they mean the
    // same thing coming back, which is a different claim and the one a rebinding
    // screen depends on.
    let ids = (DigitalId(4), AnalogId(1), PoseId(2), SetId(2));
    let text = serde_json::to_string(&ids).unwrap();
    assert_eq!(serde_json::from_str::<'_, _>(&text).ok(), Some(ids));

    let value = (
        Digital {
            held: true,
            pressed: false,
            released: true,
        },
        Analog::new(Signed16::from_bits(-1), Signed16::from_bits(32_767)),
    );
    let text = serde_json::to_string(&value).unwrap();
    assert_eq!(serde_json::from_str::<'_, _>(&text).ok(), Some(value));
}

/// Compares a labelled fixture against its recorded row and reports every row
/// that moved at once.
///
/// One row at a time would be the obvious way to write this and the wrong one.
/// A reordered declaration moves a run of rows, and the count and the run's
/// shape are the first two things worth knowing, so both are in the message and
/// the message is formatted the way the table is written.
fn check(table: &[(&str, u16)], found: &[(&str, u16)]) {
    assert_eq!(
        table.len(),
        found.len(),
        "the table has {} rows and the declaration has {}",
        table.len(),
        found.len(),
    );

    let moved: Vec<String> = table
        .iter()
        .zip(found)
        .filter(|((_, recorded), (_, actual))| recorded != actual)
        .map(|((label, _), (_, actual))| format!("    ({label:?}, {actual}),"))
        .collect();

    assert!(
        moved.is_empty(),
        "{} of {} recorded identifiers moved, which is a wire-format break and \
         not a test to regenerate:\n{}",
        moved.len(),
        table.len(),
        moved.join("\n"),
    );
}

// ---------------------------------------------------------------------------
// The widths: what neither of the two tables above can see.

/// What this crate's types digest to under `corvid_hash`'s hasher.
///
/// The third leg, and the only one that sees an integer's **width**. Both
/// encodings above spell a value and not a declaration: `4` is `4` in a
/// self-describing format whether it came from a `u16` or a `u64`, and it is the
/// single byte `04` under `corvid_wire` for the same reason, because a varint
/// picks its marker from the number rather than from the type. So widening
/// `DigitalId`, `AnalogId`, `PoseId`, `SetId` or either axis of an [`Analog`]
/// moves not one row of either.
///
/// A hasher does see it. `corvid_hash` absorbs an integer as its declared bytes
/// and injects the count of bytes absorbed, so a `u16` and a `u32` holding the
/// same number produce different digests — and the digest is what a peer
/// actually compares, since an identifier reaches a hash trace inside somebody
/// else's state.
///
/// The values are the ones the two tables above use, row for row, so all three
/// can be read side by side.
mod widths {
    use corvid_fixed::Signed16;
    use corvid_hash::digest;
    use corvid_input::{Analog, AnalogId, Digital, DigitalId, PoseId, SetId};
    use corvid_wire::golden::{DigestRow, check_digests};

    /// The four identifiers and the two value types, digested.
    const GOLDEN_DIGESTS: &[DigestRow<'_>] = &[
        ("DigitalId(4)", 0xa300_5bde_d303_206d),
        ("AnalogId(1)", 0x2bb6_22f3_c033_806b),
        ("PoseId(2)", 0xebbb_4486_c579_7f68),
        ("SetId(2)", 0xebbb_4486_c579_7f68),
        ("Digital pressed but not held", 0x7db5_3286_be97_80da),
        ("Digital held", 0xe28c_b7ad_feb8_846e),
        ("Analog(30000, -4000)", 0xb5d6_84a0_9e21_95e0),
        ("Analog::ZERO", 0x15ad_b827_954f_a565),
    ];

    #[test]
    fn the_parts_of_a_binding_file_digest_as_they_were_recorded() {
        let pushed = Analog::new(Signed16::from_bits(30_000), Signed16::from_bits(-4_000));
        let digests = [
            digest(&DigitalId(4)),
            digest(&AnalogId(1)),
            digest(&PoseId(2)),
            digest(&SetId(2)),
            digest(&Digital {
                held: false,
                pressed: true,
                released: false,
            }),
            digest(&Digital::HELD),
            digest(&pushed),
            digest(&Analog::ZERO),
        ]
        .map(corvid_hash::Digest::to_u64);
        check_digests("a binding file's parts", GOLDEN_DIGESTS, &digests).unwrap();
    }

    #[test]
    fn an_identifiers_width_is_in_the_digest_and_in_neither_encoding() {
        // The claim the table above rests on, said once without the table. Two
        // integers holding the same number at two declared widths absorb the
        // same value and a different count of bytes, so they digest apart —
        // which is the property `DigitalId` would silently lose if it were
        // widened and only the byte and text tables were watching.
        assert_ne!(digest(&2_u16), digest(&2_u32));

        // And the other direction, so that the row is read as being about the
        // width rather than about the two types being different types:
        // `PoseId(2)` and `SetId(2)` are declared at the same width over the
        // same number, and an identifier absorbs its integer and no type tag —
        // which is why those two rows above are the same digest.
        assert_eq!(digest(&PoseId(2)), digest(&SetId(2)));
    }
}
