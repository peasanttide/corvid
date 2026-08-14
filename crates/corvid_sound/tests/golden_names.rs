//! The frozen encodings. **Changing a value in this file is a wire-format
//! break.**
//!
//! An audio frame is captured to disk by a headless run and compared against a
//! capture recorded by an older build on another machine, and its digest goes
//! into the hash trace beside it. So this crate has *two* wire formats, and they
//! are independent of each other:
//!
//! The **serialized bytes** are what a capture is. They are produced by the
//! derived `serde` implementations, so they are a function of the field
//! declaration order, of how many fields there are, and of each field's value --
//! none of which is stated anywhere as an intention. Reordering two fields of
//! different types compiles, adding a field compiles, and widening an identifier
//! compiles.
//!
//! The **digest** is what a hash trace is. It is produced by the derived
//! [`Hash`] implementations under this workspace's one hasher, so it is a
//! function of the field declaration order and of the width of every integer,
//! neither of which is stated anywhere as an intention.
//!
//! Neither of them is visible to a round trip. A serialize-then-deserialize test
//! is symmetric -- the writer and the reader are derived from the same
//! declaration and move together -- so every change listed above leaves it green
//! while changing what a capture recorded yesterday means. Nor does either table
//! cover the other, because the two encodings share nothing: `serde` writes a
//! small number as one byte whatever width it was declared at, and the hasher
//! absorbs it as its declared bytes and counts them. So an identifier *widened*
//! moves the digest half of this file and no byte row at all, while a field
//! reordered or added moves both. Each table below is the only thing in this
//! crate that can see its own half.
//!
//! So both are written down as literals, over the same fixtures. Every other
//! test in this crate compares one output to another, which catches an encoding
//! that stopped distinguishing two things and cannot catch one that
//! distinguishes them differently than when the table was recorded. This file is the other half.
//!
//! Renaming a field is the change neither of those two can see: these bytes
//! carry no names and the hasher absorbs values and never names. So there is a
//! **third** table here, over the same fixtures, holding what a self-describing
//! format writes -- every field name and every variant name, in order. A change
//! that moves none of the three has not moved anything a peer or a reviewer can
//! observe.
//!
//! If a change here is genuinely wanted, it is a new version of the format:
//! bump the crate's major version, reissue every capture recorded under the old
//! one, and say so in the changelog. Regenerating these numbers to make a red
//! test go green is never the right move -- the red test *is* the notification
//! that a capture recorded yesterday has stopped being comparable.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

// ---------------------------------------------------------------------------
// The names: what neither of the other two tables can see.

/// What a *self-describing* format writes for the same fixtures.
///
/// `corvid_wire` writes a field as its value and a variant as a number, and the
/// hasher absorbs values and never names -- so renaming `Source::occlusion`, or
/// `Bus::parent`, or `Listener::pose` moves not one byte of the table above and
/// not one digest below it. This is the table that sees it, and it is the third
/// leg of the same stool rather than a second opinion: a change that moves none
/// of the three has not moved anything a peer or a reviewer can observe.
///
/// JSON because it is a self-describing format that writes a struct as its
/// field names and an enum as its variant names, which is the shape the property
/// needs. Nothing in this workspace stores JSON and this is not a claim that
/// anything should.
#[cfg(feature = "serde")]
mod names {
    use corvid_wire::golden::{Row, check_text};

    use crate::common::fixtures::{
        every_bus, every_cue, every_frame, every_listener, every_source,
    };

    const GOLDEN_BUS_TEXT: &[Row<'_>] = &[
        (
            "Bus(1), root, full gain",
            r#"{"id":1,"parent":null,"gain":65535}"#,
        ),
        (
            "Bus(1), under master, full gain",
            r#"{"id":1,"parent":0,"gain":65535}"#,
        ),
        (
            "Bus(1), under master, half gain",
            r#"{"id":1,"parent":0,"gain":32768}"#,
        ),
        ("Bus::default()", r#"{"id":0,"parent":null,"gain":65535}"#),
    ];

    const GOLDEN_SOURCE_TEXT: &[Row<'_>] = &[
        (
            "Source::new(0, 1)",
            r#"{"id":0,"sound":1,"bus":0,"position":[0,0,0],"gain":65535,"pitch":256,"occlusion":0}"#,
        ),
        (
            "everything set",
            r#"{"id":7,"sound":2,"bus":1,"position":[262144,327680,393216],"gain":49151,"pitch":384,"occlusion":8192}"#,
        ),
        (
            "everything set, gain and occlusion exchanged",
            r#"{"id":7,"sound":2,"bus":1,"position":[262144,327680,393216],"gain":8192,"pitch":384,"occlusion":49151}"#,
        ),
    ];

    const GOLDEN_CUE_TEXT: &[Row<'_>] = &[
        (
            "Cue::new(97#0, 1)",
            r#"{"id":{"fired":97,"serial":0},"sound":1,"bus":0,"position":[0,0,0],"gain":65535,"pitch":256}"#,
        ),
        (
            "everything set, 97#1",
            r#"{"id":{"fired":97,"serial":1},"sound":3,"bus":1,"position":[-458752,524288,-589824],"gain":40959,"pitch":128}"#,
        ),
        (
            "the same cue at 97#2",
            r#"{"id":{"fired":97,"serial":2},"sound":3,"bus":1,"position":[-458752,524288,-589824],"gain":40959,"pitch":128}"#,
        ),
        (
            "97#1 heard from one metre east",
            r#"{"id":{"fired":97,"serial":1},"sound":3,"bus":1,"position":[-524288,524288,-589824],"gain":40959,"pitch":128}"#,
        ),
    ];

    const GOLDEN_LISTENER_TEXT: &[Row<'_>] = &[
        (
            "Listener::default()",
            r#"{"pose":{"position":[0,0,0],"rotation":9223090561878065152},"gain":65535}"#,
        ),
        (
            "Listener at (1, 2, 3), 0.875 gain",
            r#"{"pose":{"position":[65536,131072,196608],"rotation":9223090561878065152},"gain":57343}"#,
        ),
    ];

    const GOLDEN_FRAME_TEXT: &[Row<'_>] = &[
        (
            "AudioFrame::new()",
            r#"{"listener":{"pose":{"position":[0,0,0],"rotation":9223090561878065152},"gain":65535},"sources":[],"cues":[],"buses":[]}"#,
        ),
        (
            "one source, no cues, no buses",
            r#"{"listener":{"pose":{"position":[0,0,0],"rotation":9223090561878065152},"gain":65535},"sources":[{"id":0,"sound":1,"bus":0,"position":[0,0,0],"gain":65535,"pitch":256,"occlusion":0}],"cues":[],"buses":[]}"#,
        ),
        (
            "no sources, one cue, no buses",
            r#"{"listener":{"pose":{"position":[0,0,0],"rotation":9223090561878065152},"gain":65535},"sources":[],"cues":[{"id":{"fired":97,"serial":0},"sound":1,"bus":0,"position":[0,0,0],"gain":65535,"pitch":256}],"buses":[]}"#,
        ),
        (
            "the fully populated frame",
            r#"{"listener":{"pose":{"position":[65536,131072,196608],"rotation":9223090561878065152},"gain":57343},"sources":[{"id":7,"sound":2,"bus":1,"position":[262144,327680,393216],"gain":49151,"pitch":384,"occlusion":8192}],"cues":[{"id":{"fired":97,"serial":1},"sound":3,"bus":1,"position":[-458752,524288,-589824],"gain":40959,"pitch":128}],"buses":[{"id":0,"parent":null,"gain":32768},{"id":1,"parent":0,"gain":16384}]}"#,
        ),
    ];

    /// Every fixture as the self-describing format writes it.
    fn written<T: serde::Serialize>(values: &[T]) -> Vec<String> {
        values
            .iter()
            .map(|value| serde_json::to_string(value).unwrap())
            .collect()
    }

    #[test]
    fn every_bus_writes_its_recorded_names() {
        check_text("Bus", GOLDEN_BUS_TEXT, &written(&every_bus())).unwrap();
    }

    #[test]
    fn every_source_writes_its_recorded_names() {
        check_text("Source", GOLDEN_SOURCE_TEXT, &written(&every_source())).unwrap();
    }

    #[test]
    fn every_cue_writes_its_recorded_names() {
        check_text("Cue", GOLDEN_CUE_TEXT, &written(&every_cue())).unwrap();
    }

    #[test]
    fn the_listener_and_the_frames_write_their_recorded_names() {
        check_text(
            "Listener",
            GOLDEN_LISTENER_TEXT,
            &written(&every_listener()),
        )
        .unwrap();
        check_text("AudioFrame", GOLDEN_FRAME_TEXT, &written(&every_frame())).unwrap();
    }
}
