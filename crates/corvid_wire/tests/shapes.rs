//! The frozen encoding of the named shapes. **Changing a value in this file is
//! a wire-format break**, on the same terms as `tests/golden.rs`, which holds
//! the scalars and the argument for why these tables exist at all.
//!
//! A struct writes no name and a variant writes its index, so what reaches the
//! wire is the payload and its position and nothing about the declaration. The
//! last two tests are the pair that says what that costs: the encoder is not a
//! constant function, and it is not injective across types either, so the
//! collisions a reader should know about are written down here rather than met
//! in a capture.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_wire::golden::{Row, check};
use serde::{Deserialize, Serialize};

/// The struct shapes, none of which writes a name of any kind.
///
/// The last row is the pair that matters most: a named struct and a tuple of
/// the same fields encode identically, so nothing in a capture says which one
/// wrote it. That is what makes this format compact and it is also why a
/// reordered field is invisible to everything except a recorded row.
const GOLDEN_STRUCTS: &[Row<'_>] = &[
    ("Unit", ""),
    ("Newtype(1u16)", "01"),
    ("Pair(1u16, 2u32)", "0102"),
    ("Named { first: 1u16, second: 2u32 }", "0102"),
    ("(1u16, 2u32)", "0102"),
];

/// The enum shapes. A variant is its index, written as a varint, and then its
/// payload.
///
/// `Two` and `Three` carry the same numbers as `Pair` and `Named` above, so the
/// one leading byte is the whole difference -- which is the statement that a
/// variant's *position* is on the wire and its name is not.
const GOLDEN_ENUMS: &[Row<'_>] = &[
    ("Shape::Nothing", "00"),
    ("Shape::One(1u16)", "0101"),
    ("Shape::Two(1u16, 2u32)", "020102"),
    ("Shape::Three { first: 1, second: 2 }", "030102"),
];

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Unit;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Newtype(u16);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Pair(u16, u32);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Named {
    first: u16,
    second: u32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Shape {
    Nothing,
    One(u16),
    Two(u16, u32),
    Three { first: u16, second: u32 },
}

#[test]
fn no_struct_writes_a_name_of_any_kind() {
    check("Unit", &GOLDEN_STRUCTS[..1], &[Unit]).unwrap();
    check("Newtype", &GOLDEN_STRUCTS[1..2], &[Newtype(1)]).unwrap();
    check("Pair", &GOLDEN_STRUCTS[2..3], &[Pair(1, 2)]).unwrap();
    check(
        "Named",
        &GOLDEN_STRUCTS[3..4],
        &[Named {
            first: 1,
            second: 2,
        }],
    )
    .unwrap();
    check("(u16, u32)", &GOLDEN_STRUCTS[4..], &[(1_u16, 2_u32)]).unwrap();
}

#[test]
fn a_variant_is_its_index_and_then_its_payload() {
    check(
        "Shape",
        GOLDEN_ENUMS,
        &[
            Shape::Nothing,
            Shape::One(1),
            Shape::Two(1, 2),
            Shape::Three {
                first: 1,
                second: 2,
            },
        ],
    )
    .unwrap();
}

#[test]
fn the_recorded_rows_are_not_all_the_same_row() {
    // Every table above is a claim that some value encoded to some bytes, and
    // the cheapest way for all of them to hold at once is for the encoder to
    // write one thing for everything. This is the line that rules that out from
    // inside the file, so a reader is not taking the tables on trust.
    let encoded: Vec<Vec<u8>> = [
        corvid_wire::encode(&1_u16).unwrap(),
        corvid_wire::encode(&0x1234_u16).unwrap(),
        corvid_wire::encode(&Shape::One(1)).unwrap(),
        corvid_wire::encode(&Shape::Two(1, 2)).unwrap(),
        corvid_wire::encode(&Some(0_u16)).unwrap(),
        corvid_wire::encode(&"crow".to_owned()).unwrap(),
    ]
    .into();
    for (index, one) in encoded.iter().enumerate() {
        for other in &encoded[index + 1..] {
            assert_ne!(one, other);
        }
    }
}

#[test]
fn two_shapes_this_format_writes_alike() {
    // The other half of the line above, and the more useful half: the encoder is
    // not a constant function, but it is not injective across types either, and
    // a reader of the tables should know which collisions are real rather than
    // discovering one in a capture.
    //
    // A width, first. A varint carries the value and not the declaration, so
    // every integer type holding a small number writes the same byte. This is
    // the collision `tests/visible.rs` is about and the reason the digest table
    // beside every byte table is not a duplicate of it.
    assert_eq!(
        corvid_wire::encode(&1_u16).unwrap(),
        corvid_wire::encode(&1_u32).unwrap(),
    );

    // And a tag against an index. `None` is tag zero and a payload-free first
    // variant is index zero, and both are now one byte, so the two are the same
    // byte string. Nothing in a capture says which type wrote it -- which was
    // already true of a struct against a tuple below, and is the general
    // property that a format carrying no type tags has.
    assert_eq!(
        corvid_wire::encode(&None::<u16>).unwrap(),
        corvid_wire::encode(&Shape::Nothing).unwrap(),
    );

    // And a sign against a magnitude, at the one width where the collision uses
    // every bit there is. Zigzag folds `i128::MIN` onto `u128::MAX` because that
    // is the only place left for it, so the most negative value of one type and
    // the largest value of the other are the same seventeen bytes -- the extreme
    // case of the `i64::MIN` row in the signed table, where a reader who checked
    // only the leading marker would see two identical widest integers.
    assert_eq!(
        corvid_wire::encode(&i128::MIN).unwrap(),
        corvid_wire::encode(&u128::MAX).unwrap(),
    );

    // What keeps that from mattering is that a decoder is told the type by its
    // caller and a capture is refused outright when the two builds describe
    // themselves differently. Neither value is ever read back as the other,
    // because nothing ever asks.
    assert_eq!(corvid_wire::decode::<Option<u16>>(&[0x00]).unwrap(), None);
    assert_eq!(
        corvid_wire::decode::<Shape>(&[0x00]).unwrap(),
        Shape::Nothing,
    );
}
