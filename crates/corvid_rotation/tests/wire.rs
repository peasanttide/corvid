//! The frozen encoding. **Changing a value in this file is a wire-format
//! break.**
//!
//! A rotation is a packed integer, and that is the whole of what a capture holds
//! of one. The *packing* — which chart is in which bits, how many bits a field
//! gets — is already frozen: `tests/determinism.rs` records the bit patterns a
//! table of poses quantizes to, and moving `FIELD_BITS` by one turns it red.
//!
//! What that table cannot see is how a pattern becomes bytes, and that is what
//! this one is for. A packed rotation uses its bits, so it is the one value in
//! this workspace whose width *is* on the wire: a `Rotation` is a marker and
//! four bytes and a `FineRotation` a marker and eight, least significant first,
//! with no wrapper around either — three separate claims, none of them stated
//! anywhere in the source, and all three about whether a capture recorded on one
//! machine is the same capture on another.
//! `to_bits` returning the right number says nothing about any of them: it is
//! one build's integer compared against one build's integer, on one machine's
//! byte order.
//!
//! The two tables fail on disjoint changes, which is the reason to have both.
//! Move `FIELD_BITS` and `determinism.rs` goes red on every pose while the
//! widths and the byte order here are untouched. Change the endianness, or wrap
//! the newtype in a struct, or widen the packed integer, and every pose still
//! quantizes to the number it always did.
//!
//! The crate's JSON tests are the third: they are the ones that would see this
//! type stop being `#[serde(transparent)]`, which this encoding cannot, because
//! a newtype and its contents write the same bytes.
//!
//! If a change here is genuinely wanted, it is a new version of the format: bump
//! the crate's major version, reissue every capture recorded under the old one,
//! and say so in the changelog.

#![cfg(feature = "serde")]
#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_rotation::{FineRotation, Rotation};
use corvid_wire::golden::{Row, check};

/// The packed 32-bit rotation: the marker `fc`, then four bytes least
/// significant first.
///
/// The second row is the one that is only about the encoding. Its value is a
/// pattern rather than a quantized rotation, and its four bytes are all
/// different, so it moves for an endianness change or a width change and for
/// nothing else — where the identity row would also move for a change to the
/// packing, which `tests/determinism.rs` owns.
const GOLDEN_ROTATIONS: &[Row<'_>] = &[
    ("Rotation::IDENTITY", "fc000208e0"),
    ("Rotation, bits 0x1234_5678", "fc78563412"),
];

/// The packed 64-bit rotation: the marker `fd`, then eight bytes, and the same
/// argument one tier up.
const GOLDEN_FINE_ROTATIONS: &[Row<'_>] = &[
    ("FineRotation::IDENTITY", "fd000000000000ff7f"),
    (
        "FineRotation, bits 0x1234_5678_9abc_def0",
        "fdf0debc9a78563412",
    ),
];

#[test]
fn the_packed_rotation_encodes_as_it_was_recorded() {
    check(
        "Rotation",
        GOLDEN_ROTATIONS,
        &[Rotation::IDENTITY, Rotation::from_bits(0x1234_5678)],
    )
    .unwrap();
}

#[test]
fn the_fine_rotation_encodes_as_it_was_recorded() {
    check(
        "FineRotation",
        GOLDEN_FINE_ROTATIONS,
        &[
            FineRotation::IDENTITY,
            FineRotation::from_bits(0x1234_5678_9abc_def0),
        ],
    )
    .unwrap();
}

#[test]
fn a_rotation_is_its_bit_pattern_and_nothing_else() {
    // A capture holds the pattern; what it means is a decision both peers make
    // out of their own source, and this is the line that records which is which.
    //
    // A packed rotation uses its bits, so it is the shape a varint is worst at:
    // five bytes for a four-byte pattern and nine for an eight-byte one, a
    // marker in front of a number with no leading zeroes to save.
    assert_eq!(corvid_wire::encode(&Rotation::IDENTITY).unwrap().len(), 5);
    assert_eq!(
        corvid_wire::encode(&FineRotation::IDENTITY).unwrap().len(),
        9,
    );
    assert_eq!(
        corvid_wire::encode(&Rotation::from_bits(0x1234_5678)).unwrap(),
        corvid_wire::encode(&0x1234_5678_u32).unwrap(),
    );
}

#[test]
fn widening_the_packing_moves_every_row() {
    // The two types here differ by exactly the change this file's header is
    // about: the same rotation packed at two widths. A capture written by one
    // and read by the other is not merely wrong about the rotation, it is wrong
    // about where the *next* field starts.
    //
    // This is the one widening in the workspace a byte row does see, and it sees
    // it for a reason that is about these types rather than about the encoding: a
    // packed rotation fills its width, so the wider one takes a different marker
    // and a longer payload. A widening only shows in the bytes when the value
    // outgrows the narrower type's range, and a rotation always has.
    let narrow = corvid_wire::encode(&Rotation::IDENTITY).unwrap();
    let wide = corvid_wire::encode(&FineRotation::IDENTITY).unwrap();
    assert_ne!(narrow, wide);
    assert_ne!(
        narrow[0], wide[0],
        "the marker is the first thing that moves"
    );

    // And the digest, which sees it whatever the value is.
    assert_ne!(
        corvid_hash::digest(&Rotation::IDENTITY),
        corvid_hash::digest(&FineRotation::IDENTITY),
    );
}

#[test]
fn a_non_canonical_pattern_survives_as_the_pattern_it_is() {
    // A rotation that arrived over a wire may hold a pattern this crate's own
    // encoder would not have produced, and `Eq` folds some of those onto the
    // rotation they denote. So a round trip can say a capture came back
    // unchanged when the bytes did not: this is the direction only a byte row
    // sees, and the rows above are recorded from patterns rather than from
    // quantized rotations for exactly that reason.
    let raw = Rotation::from_bits(0x1234_5678);
    let back: Rotation = corvid_wire::decode(&corvid_wire::encode(&raw).unwrap()).unwrap();
    assert_eq!(back.to_bits(), 0x1234_5678);
}
