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
// The serialized bytes: what a capture on disk is.
// ---------------------------------------------------------------------------

/// The `serde` half.
///
/// Each row is a label and the bytes the derived `Serialize` writes, as
/// lowercase hex. The rows are checked in both directions: today's encoder must
/// write these bytes, and these bytes -- which is what a capture recorded by an
/// older build holds -- must read back as the value they were recorded for.
#[cfg(feature = "serde")]
mod wire {
    use corvid_wire::golden::{Row, check};

    use crate::common::fixtures::{
        BUS_IDS, CUE_IDS, SOUND_IDS, SOURCE_IDS, every_bus, every_cue, every_frame, every_listener,
        every_source,
    };

    /// The identifiers, which are transparent newtypes: an identifier is its
    /// integer, at its declared width, and nothing else.
    const GOLDEN_SOUND_ID_BYTES: &[Row<'_>] = &[
        ("SoundId(0)", "00"),
        ("SoundId(1)", "01"),
        ("SoundId(2)", "02"),
        ("SoundId(u32::MAX)", "fcffffffff"),
    ];

    const GOLDEN_BUS_ID_BYTES: &[Row<'_>] = &[
        ("BusId::MASTER", "00"),
        ("BusId(1)", "01"),
        ("BusId(u16::MAX)", "fbffff"),
    ];

    const GOLDEN_SOURCE_ID_BYTES: &[Row<'_>] =
        &[("SourceId(7)", "07"), ("SourceId(u32::MAX)", "fcffffffff")];

    /// A cue identity is a tick and then a serial, in that order and with
    /// nothing between them. The saturated row is the one where each half's
    /// marker shows; the small ones are a byte apiece and would read the same
    /// under either width.
    const GOLDEN_CUE_ID_BYTES: &[Row<'_>] = &[
        ("CueId(97#0)", "6100"),
        ("CueId(97#1)", "6101"),
        ("CueId(98#0)", "6200"),
        ("CueId(u64::MAX#u16::MAX)", "fdfffffffffffffffffbffff"),
    ];

    /// The buses. The third byte is the `Option` tag, and it is the only
    /// difference between the first two rows.
    const GOLDEN_BUS_BYTES: &[Row<'_>] = &[
        ("Bus(1), root, full gain", "0100fbffff"),
        ("Bus(1), under master, full gain", "010100fbffff"),
        ("Bus(1), under master, half gain", "010100fb0080"),
        ("Bus::default()", "0000fbffff"),
    ];

    const GOLDEN_SOURCE_BYTES: &[Row<'_>] = &[
        ("Source::new(0, 1)", "000100000000fbfffffb000200"),
        (
            "everything set",
            "070201fc00000800fc00000a00fc00000c00fbffbffb0003fb0020",
        ),
        (
            "everything set, gain and occlusion exchanged",
            "070201fc00000800fc00000a00fc00000c00fb0020fb0003fbffbf",
        ),
    ];

    const GOLDEN_CUE_BYTES: &[Row<'_>] = &[
        ("Cue::new(97#0, 1)", "61000100000000fbfffffb0002"),
        (
            "everything set, 97#1",
            "61010301fcffff0d00fc00001000fcffff1100fbff9ffb0001",
        ),
        (
            "the same cue at 97#2",
            "61020301fcffff0d00fc00001000fcffff1100fbff9ffb0001",
        ),
        (
            "97#1 heard from one metre east",
            "61010301fcffff0f00fc00001000fcffff1100fbff9ffb0001",
        ),
    ];

    const GOLDEN_LISTENER_BYTES: &[Row<'_>] = &[
        ("Listener::default()", "000000fd000000000000ff7ffbffff"),
        (
            "Listener at (1, 2, 3), 0.875 gain",
            "fc00000200fc00000400fc00000600fd000000000000ff7ffbffdf",
        ),
    ];

    const GOLDEN_FRAME_BYTES: &[Row<'_>] = &[
        ("AudioFrame::new()", "000000fd000000000000ff7ffbffff000000"),
        (
            "one source, no cues, no buses",
            "000000fd000000000000ff7ffbffff01000100000000fbfffffb0002000000",
        ),
        (
            "no sources, one cue, no buses",
            "000000fd000000000000ff7ffbffff000161000100000000fbfffffb000200",
        ),
        (
            "the fully populated frame",
            "fc00000200fc00000400fc00000600fd000000000000ff7ffbffdf01070201fc00000800fc00000a00fc00000c00fbffbffb0003fb00200161010301fcffff0d00fc00001000fcffff1100fbff9ffb0001020000fb0080010100fb0040",
        ),
    ];

    #[test]
    fn the_identifiers_serialize_to_their_recorded_bytes() {
        check("SoundId", GOLDEN_SOUND_ID_BYTES, SOUND_IDS).unwrap();
        check("BusId", GOLDEN_BUS_ID_BYTES, BUS_IDS).unwrap();
        check("SourceId", GOLDEN_SOURCE_ID_BYTES, SOURCE_IDS).unwrap();
        check("CueId", GOLDEN_CUE_ID_BYTES, CUE_IDS).unwrap();
    }

    #[test]
    fn every_bus_serializes_to_its_recorded_bytes() {
        check("Bus", GOLDEN_BUS_BYTES, &every_bus()).unwrap();
    }

    #[test]
    fn every_source_serializes_to_its_recorded_bytes() {
        check("Source", GOLDEN_SOURCE_BYTES, &every_source()).unwrap();
    }

    #[test]
    fn every_cue_serializes_to_its_recorded_bytes() {
        check("Cue", GOLDEN_CUE_BYTES, &every_cue()).unwrap();
    }

    #[test]
    fn the_listener_and_the_frames_serialize_to_their_recorded_bytes() {
        check("Listener", GOLDEN_LISTENER_BYTES, &every_listener()).unwrap();
        check("AudioFrame", GOLDEN_FRAME_BYTES, &every_frame()).unwrap();
    }
}
