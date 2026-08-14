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
// The digests: what a hash trace is.
// ---------------------------------------------------------------------------

/// The digest half.
mod digests {
    use corvid_hash::{Digest, digest};
    use corvid_sound::{BusId, SoundId};
    use corvid_wire::golden::{DigestRow, check_digests};

    use crate::common::fixtures::{
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
        // different kinds apart is the width of the integer each holds -- a
        // `SoundId` is four bytes and a `BusId` is two, in the digest exactly as
        // in the serialized bytes. What establishes that two peers are reading
        // the same field at all is the opening's schema rather than anything on
        // the value.
        assert_ne!(digest(&SoundId(1)), digest(&BusId(1)));

        // Which number the master bus actually is. The table catches a change --
        // `check` zips it positionally, so moving `MASTER` off zero moves that
        // row -- but it catches it as a wire-format break, and a reader who came
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
