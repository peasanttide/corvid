//! The frozen encoding. **Changing a value in this file is a wire-format
//! break.**
//!
//! A transform is where an entity is and which way it faces, and it is the field
//! a snapshot holds most of. Unlike everything underneath it in this stack, a
//! transform is a *struct* rather than a transparent newtype: it has two fields
//! and they are written in declaration order, with no name and no tag.
//!
//! The field order is already covered — `tests/interop.rs` pins the JSON, which
//! carries the names and would go red the moment `position` and `rotation`
//! exchanged places or one of them was renamed. What this table adds is
//! everything about the same two fields that JSON has no way to spell: the exact
//! bytes each coordinate becomes, and that there is no padding, no discriminant
//! and nothing at all between the position and the rotation, so a reader on the
//! other end computes the second field's offset from the first field's bytes
//! rather than looking it up.
//!
//! What separates the two tiers here is the *rotation*. A packed rotation fills
//! its width, so the coarse one takes the marker `fc` and four bytes and the fine
//! one `fd` and eight. The two positions do not separate: a coordinate is written
//! as a varint, so the same small numbers are the same bytes whether they were
//! declared at four bytes each or eight. A widened coordinate is invisible here
//! and invisible in JSON, and what sees it is the digest — `tests/determinism.rs`
//! is that table.
//!
//! So the tables are all needed and none substitutes for another: JSON sees the
//! names, this sees the values and the boundary, and the digest sees the width.
//!
//! If a change here is genuinely wanted, it is a new version of the format: bump
//! the crate's major version, reissue every capture recorded under the old one,
//! and say so in the changelog.

#![cfg(feature = "serde")]
#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::{I24F8, I48F16};
use corvid_rotation::{FineRotation, Rotation};
use corvid_transform::{GlobalFineTransform, Transform};
use corvid_vector::{GlobalFinePoint, GlobalPoint};
use corvid_wire::golden::{Row, check};

/// The object tier: twelve bytes of position, then four of rotation.
///
/// The second row is what makes the order visible. Its position and its rotation
/// hold different numbers and neither is zero, so a transform that started
/// writing its rotation first moves the row; a row recorded from the identity
/// alone would not, because the identity's position is zeroes and a reader
/// cannot tell twelve zero bytes at the front from twelve zero bytes further
/// along.
const GOLDEN_TRANSFORMS: &[Row<'_>] = &[
    ("Transform::IDENTITY", "000000fc000208e0"),
    (
        "Transform, position bits 1, 2, -3, rotation bits 0x1234_5678",
        "020405fc78563412",
    ),
];

/// The camera and tracked-pose tier: twenty-four bytes of position, then eight
/// of rotation.
const GOLDEN_FINE_TRANSFORMS: &[Row<'_>] = &[
    ("GlobalFineTransform::IDENTITY", "000000fd000000000000ff7f"),
    (
        "GlobalFineTransform, position bits 1, 2, -3, rotation bits 0x1234_5678_9abc_def0",
        "020405fdf0debc9a78563412",
    ),
];

const fn transform() -> Transform {
    Transform::new(
        GlobalPoint::new(
            I24F8::from_bits(1),
            I24F8::from_bits(2),
            I24F8::from_bits(-3),
        ),
        Rotation::from_bits(0x1234_5678),
    )
}

const fn fine_transform() -> GlobalFineTransform {
    GlobalFineTransform::new(
        GlobalFinePoint::new(
            I48F16::from_bits(1),
            I48F16::from_bits(2),
            I48F16::from_bits(-3),
        ),
        FineRotation::from_bits(0x1234_5678_9abc_def0),
    )
}

#[test]
fn the_object_transform_encodes_as_it_was_recorded() {
    check(
        "Transform",
        GOLDEN_TRANSFORMS,
        &[Transform::IDENTITY, transform()],
    )
    .unwrap();
}

#[test]
fn the_fine_transform_encodes_as_it_was_recorded() {
    check(
        "GlobalFineTransform",
        GOLDEN_FINE_TRANSFORMS,
        &[GlobalFineTransform::IDENTITY, fine_transform()],
    )
    .unwrap();
}

#[test]
fn a_transform_is_its_position_and_then_its_rotation() {
    // The claim the rows above make, spelled out where a reader can check it:
    // the bytes of a transform are the bytes of its position followed by the
    // bytes of its rotation, with no tag, no count and no padding between them.
    // `tests/interop.rs` says the two fields are named and in that order; this
    // says where the boundary between them falls, which is the part a reader on
    // the other end of a capture has to compute rather than look up.
    let whole = corvid_wire::encode(&transform()).unwrap();
    let position = corvid_wire::encode(&transform().position()).unwrap();
    let rotation = corvid_wire::encode(&transform().rotation()).unwrap();

    assert_eq!(whole, [position.clone(), rotation.clone()].concat());
    assert_ne!(whole, [rotation.clone(), position.clone()].concat());

    // The boundary itself, as two numbers. A packed rotation fills its width, so
    // it always takes a marker and its whole four-byte payload; the position in
    // front of it is whatever its three coordinates cost as varints, which for
    // this fixture's small ones is a byte each.
    assert_eq!(position.len(), 3);
    assert_eq!(rotation.len(), 5);
    assert_eq!(whole.len(), 8);
}

#[test]
fn the_two_tiers_are_the_same_shape_at_two_widths() {
    // The widening this file's header is about, as the two types that already
    // differ by it. Both hold the same three coordinates and both are a position
    // and then a rotation, and the *position* halves of the two are the same
    // bytes: a varint carries a coordinate's value and not the width it was
    // declared at, so widening one is invisible here.
    let narrow = corvid_wire::encode(&transform()).unwrap();
    let wide = corvid_wire::encode(&fine_transform()).unwrap();
    let narrow_position = corvid_wire::encode(&transform().position()).unwrap();
    let wide_position = corvid_wire::encode(&fine_transform().position()).unwrap();
    assert_eq!(narrow_position, wide_position);

    // What separates the two whole transforms is the rotation, which fills its
    // width and so takes a wider marker — five bytes against nine.
    assert_ne!(narrow, wide);
    assert_eq!(wide.len(), narrow.len() + 4);

    // And the digest, which sees the position's width as well, because the
    // hasher absorbs a coordinate as its declared bytes.
    assert_ne!(
        corvid_hash::digest(&transform().position()),
        corvid_hash::digest(&fine_transform().position()),
    );
}
