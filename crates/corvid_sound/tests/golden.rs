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
//! declaration order, of how many fields there are, and of each field's value —
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
//! is symmetric — the writer and the reader are derived from the same
//! declaration and move together — so every change listed above leaves it green
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
//! format writes — every field name and every variant name, in order. A change
//! that moves none of the three has not moved anything a peer or a reviewer can
//! observe.
//!
//! If a change here is genuinely wanted, it is a new version of the format:
//! bump the crate's major version, reissue every capture recorded under the old
//! one, and say so in the changelog. Regenerating these numbers to make a red
//! test go green is never the right move — the red test *is* the notification
//! that a capture recorded yesterday has stopped being comparable.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use corvid_fixed::{Factor16, I8F8, I16F16, I48F16};
use corvid_sound::{AudioFrame, Bus, BusId, Cue, CueId, Listener, SoundId, Source, SourceId};
use corvid_time::Tick;
use corvid_transform::FineTransform;
use corvid_vector::{FinePoint, GlobalFinePoint};

// ---------------------------------------------------------------------------
// The fixtures, shared by both halves.
// ---------------------------------------------------------------------------

/// Every identifier the tables below cover, including the widest value each
/// type can hold.
///
/// The saturated rows are not decoration. A varint spells a small number the
/// same however it was declared, so a row holding 0 or 1 says nothing about an
/// identifier's width at all — the saturated row is where the marker and the
/// four bytes of `ff` appear, and it is the only byte row a widening or a
/// narrowing runs into.
const SOUND_IDS: &[SoundId] = &[SoundId(0), SoundId(1), SoundId(2), SoundId(u32::MAX)];
const BUS_IDS: &[BusId] = &[BusId::MASTER, BusId(1), BusId(u16::MAX)];
const SOURCE_IDS: &[SourceId] = &[SourceId(7), SourceId(u32::MAX)];

/// The cue identities, whose two halves have different widths.
const CUE_IDS: &[CueId] = &[
    CueId::new(Tick(97), 0),
    CueId::new(Tick(97), 1),
    CueId::new(Tick(98), 0),
    CueId::new(Tick(u64::MAX), u16::MAX),
];

/// A source with a different value in every field.
const fn full_source() -> Source {
    Source::new(SourceId(7), SoundId(2))
        .on(BusId(1))
        .at(FinePoint::new(
            I16F16::from_f64(4.0),
            I16F16::from_f64(5.0),
            I16F16::from_f64(6.0),
        ))
        .with_gain(Factor16::from_f64(0.75))
        .with_pitch(I8F8::from_f64(1.5))
        .occluded_by(Factor16::from_f64(0.125))
}

/// A cue with a different value in every field.
const fn full_cue() -> Cue {
    Cue::new(CueId::new(Tick(97), 1), SoundId(3))
        .on(BusId(1))
        .at(FinePoint::new(
            I16F16::from_f64(-7.0),
            I16F16::from_f64(8.0),
            I16F16::from_f64(-9.0),
        ))
        .with_gain(Factor16::from_f64(0.625))
        .with_pitch(I8F8::from_f64(0.5))
}

const fn full_listener() -> Listener {
    Listener::new(FineTransform::IDENTITY.with_position(GlobalFinePoint::new(
        I48F16::from_f64(1.0),
        I48F16::from_f64(2.0),
        I48F16::from_f64(3.0),
    )))
    .with_gain(Factor16::from_f64(0.875))
}

/// The buses, whose three fields are emitted in declaration order.
///
/// The first two rows are the `Option` pair: a root bus and a bus parented to
/// bus zero. They differ in one byte of the serialized form and one word of the
/// digest, and that byte and that word are the whole of what keeps the master
/// bus distinguishable from a bus feeding it.
fn every_bus() -> Vec<Bus> {
    vec![
        Bus::new(BusId(1)),
        Bus::new(BusId(1)).under(BusId::MASTER),
        Bus::new(BusId(1))
            .under(BusId::MASTER)
            .with_gain(Factor16::from_f64(0.5)),
        Bus::default(),
    ]
}

/// The sources, whose seven fields are emitted in declaration order.
///
/// Every field of the second row holds a value no other field of that row
/// holds, which is what makes the order visible: a source that emitted its
/// fields backwards would still encode to something, and would still tell two
/// different sources apart, and would still pass every relative test here. The
/// third row is that source with `gain` and `occlusion` exchanged.
fn every_source() -> Vec<Source> {
    let full = full_source();
    vec![
        Source::new(SourceId(0), SoundId(1)),
        full,
        full.with_gain(full.occlusion).occluded_by(full.gain),
    ]
}

/// The cues, whose identity is emitted before their payload.
///
/// The last three rows are the three-way distinction the whole crate turns on,
/// written out so that each half of it is frozen. Rows two and three are the
/// same payload under two identities — a second bounce on the same tick — and
/// rows two and four are the same identity under two payloads — one bounce
/// heard from two places as the listener walked. All three differ, which is what
/// makes an encoding a change detector and not an identity, and an identity not
/// a change detector. Both are in the frame because neither can do the other's
/// job.
fn every_cue() -> Vec<Cue> {
    let full = full_cue();
    vec![
        Cue::new(CueId::first(Tick(97)), SoundId(1)),
        full,
        Cue {
            id: CueId::new(Tick(97), 2),
            ..full
        },
        full.at(full
            .position
            .sub(FinePoint::new(I16F16::ONE, I16F16::ZERO, I16F16::ZERO))),
    ]
}

fn every_listener() -> Vec<Listener> {
    vec![Listener::default(), full_listener()]
}

/// The frames.
///
/// The empty frame is the row that pins the three list lengths: without them, a
/// frame with nothing in it would be its listener and stop, and a later format
/// that added a fourth list would collide with it.
fn every_frame() -> Vec<AudioFrame> {
    let mut one_source = AudioFrame::new();
    one_source.source(Source::new(SourceId(0), SoundId(1)));

    let mut one_cue = AudioFrame::new();
    one_cue.cue(Cue::new(CueId::first(Tick(97)), SoundId(1)));

    vec![AudioFrame::new(), one_source, one_cue, populated()]
}

/// One of everything, which is the shape a captured frame actually has.
fn populated() -> AudioFrame {
    let mut frame = AudioFrame::new();
    frame.listen(full_listener());
    frame.bus(Bus::new(BusId::MASTER).with_gain(Factor16::from_f64(0.5)));
    frame.bus(
        Bus::new(BusId(1))
            .under(BusId::MASTER)
            .with_gain(Factor16::from_f64(0.25)),
    );
    frame.source(full_source());
    frame.cue(full_cue());
    frame
}

// ---------------------------------------------------------------------------
// The serialized bytes: what a capture on disk is.
// ---------------------------------------------------------------------------

/// The `serde` half.
///
/// Each row is a label and the bytes the derived `Serialize` writes, as
/// lowercase hex. The rows are checked in both directions: today's encoder must
/// write these bytes, and these bytes — which is what a capture recorded by an
/// older build holds — must read back as the value they were recorded for.
#[cfg(feature = "serde")]
mod wire {
    use corvid_wire::golden::{Row, check};

    use super::{
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

// ---------------------------------------------------------------------------
// The names: what neither of the other two tables can see.

/// What a *self-describing* format writes for the same fixtures.
///
/// `corvid_wire` writes a field as its value and a variant as a number, and the
/// hasher absorbs values and never names — so renaming `Source::occlusion`, or
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

    use super::{every_bus, every_cue, every_frame, every_listener, every_source};

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

// ---------------------------------------------------------------------------
// The digests: what a hash trace is.
// ---------------------------------------------------------------------------

/// The digest half.
mod digests {
    use corvid_hash::{Digest, digest};
    use corvid_sound::{BusId, SoundId};
    use corvid_wire::golden::{DigestRow, check_digests};

    use super::{
        BUS_IDS, CUE_IDS, SOUND_IDS, SOURCE_IDS, every_bus, every_cue, every_frame, every_listener,
        every_source,
    };

    /// The identifiers, which absorb their integer and no type tag.
    ///
    /// The `MASTER` row is here rather than only in the bus table because it is
    /// a number that leaves this crate: a backend reads it to decide which bus
    /// is the root, and every relative comparison in the workspace compares it
    /// only to other bus identifiers, so a change from zero to one would move
    /// nothing else.
    const GOLDEN_SOUND_IDS: &[DigestRow<'_>] = &[
        ("SoundId(0)", 0x2e28_3c10_6fdf_99ad),
        ("SoundId(1)", 0xd2ad_74d3_e9bb_9f8b),
        ("SoundId(2)", 0xd501_4a06_3c01_9f99),
        ("SoundId(u32::MAX)", 0x0490_d681_3d26_d063),
    ];

    const GOLDEN_BUS_IDS: &[DigestRow<'_>] = &[
        ("BusId::MASTER", 0xa84f_c15c_a001_a03e),
        ("BusId(1)", 0x2bb6_22f3_c033_806b),
        ("BusId(u16::MAX)", 0xc854_ebe8_08b5_aff8),
    ];

    const GOLDEN_SOURCE_IDS: &[DigestRow<'_>] = &[
        ("SourceId(7)", 0x61db_1701_366b_9056),
        ("SourceId(u32::MAX)", 0x0490_d681_3d26_d063),
    ];

    const GOLDEN_CUE_IDS: &[DigestRow<'_>] = &[
        ("CueId(97#0)", 0x86bb_7db2_32f2_88ce),
        ("CueId(97#1)", 0x9db0_6b05_9be9_0bdd),
        ("CueId(98#0)", 0x71c4_77ed_47f1_81f9),
        ("CueId(u64::MAX#u16::MAX)", 0x75fb_b96c_6265_e419),
    ];

    const GOLDEN_BUSES: &[DigestRow<'_>] = &[
        ("Bus(1), root, full gain", 0x706f_dbc7_dc48_8c5e),
        ("Bus(1), under master, full gain", 0xb600_f53a_72df_98d3),
        ("Bus(1), under master, half gain", 0x1373_0482_dad1_6adf),
        ("Bus::default()", 0x576b_9f86_685e_1eab),
    ];

    const GOLDEN_SOURCES: &[DigestRow<'_>] = &[
        ("Source::new(0, 1)", 0xe3be_967f_aa93_b860),
        ("everything set", 0x699c_f640_a81b_9435),
        (
            "everything set, gain and occlusion exchanged",
            0x1ddc_2d17_8033_fb84,
        ),
    ];

    const GOLDEN_CUES: &[DigestRow<'_>] = &[
        ("Cue::new(97#0, 1)", 0x7e66_2efc_3536_f753),
        ("everything set, 97#1", 0x3433_2bf0_74e1_a9ca),
        ("the same cue at 97#2", 0x03a0_00f3_92d7_ded2),
        ("97#1 heard from one metre east", 0x94ae_060c_57d9_1ee8),
    ];

    const GOLDEN_LISTENERS: &[DigestRow<'_>] = &[
        ("Listener::default()", 0xc5cc_eb6b_f556_5cef),
        ("Listener at (1, 2, 3), 0.875 gain", 0x4fa4_8645_599f_52c3),
    ];

    const GOLDEN_FRAMES: &[DigestRow<'_>] = &[
        ("AudioFrame::new()", 0xad6a_4a96_5000_9c94),
        ("one source, no cues, no buses", 0xa358_8afc_a7cd_5131),
        ("no sources, one cue, no buses", 0x7659_f0e6_8060_b290),
        ("the fully populated frame", 0x999d_f76a_e677_4615),
    ];

    #[test]
    fn the_identifiers_digest_to_their_recorded_values() {
        check("SoundId", GOLDEN_SOUND_IDS, &digests(SOUND_IDS));
        check("BusId", GOLDEN_BUS_IDS, &digests(BUS_IDS));
        check("SourceId", GOLDEN_SOURCE_IDS, &digests(SOURCE_IDS));
        check("CueId", GOLDEN_CUE_IDS, &digests(CUE_IDS));

        // Nothing absorbs a type tag, so what keeps two identifiers of
        // different kinds apart is the width of the integer each holds — a
        // `SoundId` is four bytes and a `BusId` is two, in the digest exactly as
        // in the serialized bytes. What establishes that two peers are reading
        // the same field at all is the opening's schema rather than anything on
        // the value.
        assert_ne!(digest(&SoundId(1)), digest(&BusId(1)));

        // Which number the master bus actually is. The table catches a change —
        // `check` zips it positionally, so moving `MASTER` off zero moves that
        // row — but it catches it as a wire-format break, and a reader who came
        // here expecting a changed encoding is being sent to the wrong question.
        assert_eq!(BusId::MASTER, BusId(0));
    }

    #[test]
    fn every_bus_digests_to_its_recorded_value() {
        check("Bus", GOLDEN_BUSES, &digests(&every_bus()));
    }

    #[test]
    fn every_source_digests_to_its_recorded_value() {
        check("Source", GOLDEN_SOURCES, &digests(&every_source()));
    }

    #[test]
    fn every_cue_digests_to_its_recorded_value() {
        check("Cue", GOLDEN_CUES, &digests(&every_cue()));
    }

    #[test]
    fn the_listener_and_the_frames_digest_to_their_recorded_values() {
        check("Listener", GOLDEN_LISTENERS, &digests(&every_listener()));
        check("AudioFrame", GOLDEN_FRAMES, &digests(&every_frame()));
    }

    /// The digests of a fixture, in order.
    fn digests<T: core::hash::Hash>(values: &[T]) -> Vec<Digest> {
        values.iter().map(|value| digest(value)).collect()
    }

    /// The workspace's digest-table comparison, over this crate's `Digest`.
    ///
    /// `corvid_wire::golden::check_digests` is the comparison itself: it
    /// reports every row that moved at once, as paste-ready literals, because a
    /// deliberate format change moves every row and an accidental one usually
    /// moves a handful. It lived here until three crates had grown their own
    /// copy of it and the three had started to drift. What is left is turning a
    /// slice of `Digest` into the `u64`s it takes, and a moved row into a
    /// failed test.
    fn check(what: &str, table: &[DigestRow<'_>], digests: &[Digest]) {
        let bits: Vec<u64> = digests.iter().map(|digest| digest.to_u64()).collect();
        check_digests(what, table, &bits).unwrap();
    }
}
