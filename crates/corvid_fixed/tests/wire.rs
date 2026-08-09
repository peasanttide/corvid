//! The frozen encoding. **Changing a value in this file is a wire-format
//! break.**
//!
//! Every scalar in this crate ends up inside somebody's snapshot. A position is
//! three `I24F8`s, a rotation is packed into an integer, a volume is a
//! `Factor16` and a tick rate is a count -- so the bytes recorded here are most
//! of the bytes of a save file, and all of them are written by a derive that
//! nothing in the source constrains.
//!
//! What that derive writes down is the *width* of the newtype's representation,
//! and only that: every type here is `#[serde(transparent)]`, so an `Angle16` is
//! two bytes rather than a wrapper around two bytes, and its bytes are the bytes
//! of a bare `u16`. That transparency is worth freezing on its own -- losing it
//! would add nothing visible to a JSON table and would change every capture in
//! the workspace -- and so is the width.
//!
//! The width is what a recorded *digest* is the witness to, and no byte row
//! here sees it. Whether a widening is a compile error depends entirely on how
//! it arrives: changing `Angle16(u16)` to `Angle16(u32)` leaves this crate's
//! *library* compiling untouched, and is caught only by call sites that spell
//! the old width out -- and a widening reached another way, through a composition
//! that swapped one fixed-point scalar for a wider one or through a value only
//! ever built from `f64`, has no such call sites at all. What does not depend on
//! the route is that a round trip stays green, because the writer and the reader
//! are derived from one declaration and move together; that a JSON table stays
//! green, because JSON writes `4` for a `u8` and for a `u64` alike; and that the
//! byte rows below stay green too, because a varint spells a small number the
//! same at either width. What moves is the digest, which absorbs an integer as
//! its declared bytes and injects the count --
//! [`widening_a_scalar_moves_the_digest_and_not_the_bytes`] is that pair of
//! facts, and `tests/determinism.rs` is the table it argues for.
//!
//! So this file's three companions each own a different question and none
//! substitutes for another: JSON sees a field renamed, these bytes see a value
//! and a field order, and the digest sees a width.
//!
//! [`widening_a_scalar_moves_the_digest_and_not_the_bytes`]:
//!     widening_a_scalar_moves_the_digest_and_not_the_bytes
//!
//! If a change here is genuinely wanted, it is a new version of the format: bump
//! the crate's major version, reissue every capture recorded under the old one,
//! and say so in the changelog. Regenerating these literals to make a red test
//! go green is never the right move.

#![cfg(feature = "serde")]
#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::{
    Angle8, Angle16, Angle32, Factor8, Factor16, Factor32, I0F8, I2F30, I8F8, I16F16, I24F8,
    I48F16, Pitch8, Pitch16, Pitch32, Signed8, Signed16, Signed32,
};
use corvid_wire::golden::{Row, check};
use serde::Serialize;

/// The three angles, which are unsigned fractions of a turn.
///
/// Every value that fits in more than one byte is chosen so that its bytes are
/// not all alike, which is what makes an endianness change move the row rather
/// than leave it where it was.
const GOLDEN_ANGLES: &[Row<'_>] = &[
    ("Angle8(0x12)", "12"),
    ("Angle8, every bit set", "ff"),
    ("Angle16(0x1234)", "fb3412"),
    ("Angle16, every bit set", "fbffff"),
    ("Angle32(0x1234_5678)", "fc78563412"),
];

/// The three factors, which are unsigned and share the angles' widths.
///
/// They are here as well as the angles because the two families are different
/// types with the same representation, and a table that covered one and not the
/// other would go green on a change that widened the family it skipped.
const GOLDEN_FACTORS: &[Row<'_>] = &[
    ("Factor8(0x12)", "12"),
    ("Factor16(0x1234)", "fb3412"),
    ("Factor32(0x1234_5678)", "fc78563412"),
];

/// The three pitches, which are signed.
///
/// The negative rows are what pin two's complement at the declared width. A
/// format that widened before writing, or that wrote a sign and a magnitude,
/// moves every one of them and leaves the positive rows alone.
const GOLDEN_PITCHES: &[Row<'_>] = &[
    ("Pitch8(-2)", "fe"),
    ("Pitch16(0x1234)", "fb6824"),
    ("Pitch16(-2)", "03"),
    ("Pitch32(0x1234_5678)", "fcf0ac6824"),
];

/// The six fixed-point scalars, which are signed and which a position is made
/// of.
///
/// `I24F8` and `I16F16` are both `i32` and encode identically, and `I2F30` is a
/// third. That is the convention rather than an oversight -- where the point sits
/// is the type's business and not the wire's -- but it is also why nothing in a
/// capture says which of the three wrote it, and why a field that changed from
/// one to another is a change *no* table in this workspace can see. It is a
/// reinterpretation of the same bytes, and the schema on both peers is what says
/// which reading is right.
const GOLDEN_POINTS: &[Row<'_>] = &[
    ("I0F8(-2)", "fe"),
    ("I8F8(0x1234)", "fb6824"),
    ("I8F8(-2)", "03"),
    ("I24F8(0x1234_5678)", "fcf0ac6824"),
    ("I16F16(0x1234_5678)", "fcf0ac6824"),
    ("I16F16(-2)", "03"),
    ("I2F30(0x1234_5678)", "fcf0ac6824"),
    ("I48F16(0x1234_5678_9abc_def0)", "fde0bd7935f1ac6824"),
    ("I48F16(-2)", "03"),
];

/// The three signed-normalized scalars, and the pair of rows that only a byte
/// table can hold apart.
///
/// The last two are both `-1.0`. `Signed32` has a redundant encoding for it, the
/// crate's own comparison folds the two together, and so every value comparison
/// in this workspace -- including the read-back half of `check` itself -- says
/// they are the same value. The bytes say otherwise, and the bytes are what a
/// capture holds: two peers that disagree about which pattern to write are two
/// peers whose captures differ byte for byte while every assertion they could
/// make about them passes.
const GOLDEN_SIGNED: &[Row<'_>] = &[
    ("Signed8(-2)", "fe"),
    ("Signed16(0x1234)", "fb6824"),
    ("Signed32(0x1234_5678)", "fcf0ac6824"),
    ("Signed32, the denormal -1.0", "fcffffffff"),
    ("Signed32, the normal -1.0", "fcfdffffff"),
];

#[test]
fn the_angles_encode_as_they_were_recorded() {
    check(
        "Angle8",
        &GOLDEN_ANGLES[..2],
        &[Angle8::from_bits(0x12), Angle8::from_bits(u8::MAX)],
    )
    .unwrap();
    check(
        "Angle16",
        &GOLDEN_ANGLES[2..4],
        &[Angle16::from_bits(0x1234), Angle16::from_bits(u16::MAX)],
    )
    .unwrap();
    check(
        "Angle32",
        &GOLDEN_ANGLES[4..],
        &[Angle32::from_bits(0x1234_5678)],
    )
    .unwrap();
}

#[test]
fn the_factors_encode_as_they_were_recorded() {
    check("Factor8", &GOLDEN_FACTORS[..1], &[Factor8::from_bits(0x12)]).unwrap();
    check(
        "Factor16",
        &GOLDEN_FACTORS[1..2],
        &[Factor16::from_bits(0x1234)],
    )
    .unwrap();
    check(
        "Factor32",
        &GOLDEN_FACTORS[2..],
        &[Factor32::from_bits(0x1234_5678)],
    )
    .unwrap();
}

#[test]
fn the_pitches_encode_as_they_were_recorded() {
    check("Pitch8", &GOLDEN_PITCHES[..1], &[Pitch8::from_bits(-2)]).unwrap();
    check(
        "Pitch16",
        &GOLDEN_PITCHES[1..3],
        &[Pitch16::from_bits(0x1234), Pitch16::from_bits(-2)],
    )
    .unwrap();
    check(
        "Pitch32",
        &GOLDEN_PITCHES[3..],
        &[Pitch32::from_bits(0x1234_5678)],
    )
    .unwrap();
}

#[test]
fn the_fixed_point_scalars_encode_as_they_were_recorded() {
    check("I0F8", &GOLDEN_POINTS[..1], &[I0F8::from_bits(-2)]).unwrap();
    check(
        "I8F8",
        &GOLDEN_POINTS[1..3],
        &[I8F8::from_bits(0x1234), I8F8::from_bits(-2)],
    )
    .unwrap();
    check(
        "I24F8",
        &GOLDEN_POINTS[3..4],
        &[I24F8::from_bits(0x1234_5678)],
    )
    .unwrap();
    check(
        "I16F16",
        &GOLDEN_POINTS[4..6],
        &[I16F16::from_bits(0x1234_5678), I16F16::from_bits(-2)],
    )
    .unwrap();
    check(
        "I2F30",
        &GOLDEN_POINTS[6..7],
        &[I2F30::from_bits(0x1234_5678)],
    )
    .unwrap();
    check(
        "I48F16",
        &GOLDEN_POINTS[7..],
        &[
            I48F16::from_bits(0x1234_5678_9abc_def0),
            I48F16::from_bits(-2),
        ],
    )
    .unwrap();
}

#[test]
fn the_signed_normalized_scalars_encode_as_they_were_recorded() {
    check("Signed8", &GOLDEN_SIGNED[..1], &[Signed8::from_bits(-2)]).unwrap();
    check(
        "Signed16",
        &GOLDEN_SIGNED[1..2],
        &[Signed16::from_bits(0x1234)],
    )
    .unwrap();
    check(
        "Signed32",
        &GOLDEN_SIGNED[2..],
        &[
            Signed32::from_bits(0x1234_5678),
            Signed32::from_bits(i32::MIN),
            Signed32::from_bits(-i32::MAX),
        ],
    )
    .unwrap();
}

#[test]
fn the_two_encodings_of_minus_one_are_one_value_and_two_captures() {
    // The claim the last two golden rows rest on, stated where a reader of this
    // file can check it. The crate is right to fold these -- they denote the same
    // rotation, the same volume, the same anything -- and a capture is still two
    // different byte strings, which is what makes the rows above worth having
    // rather than a restatement of the row before them.
    let denormal = Signed32::from_bits(i32::MIN);
    let normal = Signed32::from_bits(-i32::MAX);
    assert_eq!(denormal, normal);
    assert_ne!(
        corvid_wire::encode(&denormal).unwrap(),
        corvid_wire::encode(&normal).unwrap(),
    );
}

/// A twin of `Angle16` at the next width up, holding the number `Angle16` holds.
///
/// This is how the widening claim in this file's header is checked rather than
/// asserted. The crate's own types cannot be widened from a test, so the twin
/// stands in for the edit: one `#[serde(transparent)]` newtype over a `u16` and
/// one over a `u32`, same value, and the question is which recorded table could
/// tell them apart.
#[derive(Hash, Serialize)]
#[serde(transparent)]
struct WiderAngle(u32);

#[test]
fn widening_a_scalar_moves_the_digest_and_not_the_bytes() {
    let narrow = corvid_wire::encode(&Angle16::from_bits(1)).unwrap();
    let wide = corvid_wire::encode(&WiderAngle(1)).unwrap();

    // The same single byte. A varint carries the number and not the declaration,
    // so every byte row in this file stays green through the widening -- which is
    // why the widening claim in the header belongs to the digest table.
    assert_eq!(narrow, [0x01]);
    assert_eq!(wide, [0x01]);

    // And the digest, which is not green. `corvid_hash` absorbs two bytes on one
    // side and four on the other and injects the total at the end, so a peer
    // comparing digests refuses the build that made this edit at the first tick.
    assert_ne!(
        corvid_hash::digest(&Angle16::from_bits(1)),
        corvid_hash::digest(&WiderAngle(1)),
    );
}

/// A twin of `Angle16` that is *not* transparent.
#[derive(Serialize)]
struct WrappedAngle {
    bits: u16,
}

#[test]
fn transparency_is_not_visible_in_the_bytes_and_is_pinned_anyway() {
    // A newtype struct and its contents encode alike under this format, so
    // losing `#[serde(transparent)]` on one of these families would move no row
    // here and no row of a byte table anywhere. It would move a JSON table,
    // which is the reason the crate keeps one -- this line records which of the
    // two tables owns the question.
    assert_eq!(
        corvid_wire::encode(&Angle16::from_bits(0x1234)).unwrap(),
        corvid_wire::encode(&WrappedAngle { bits: 0x1234 }).unwrap(),
    );
}
