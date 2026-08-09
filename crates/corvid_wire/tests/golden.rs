//! The frozen encoding. **Changing a value in this file is a wire-format
//! break.**
//!
//! Every other byte golden in this workspace is written in terms of this one, so
//! this is the table that has to hold still first. It is not a test of the crate
//! that implements the encoder -- that crate has its own -- but of the
//! configuration this crate picked, which is a decision recorded nowhere else:
//! an upgrade that changed a length prefix's width, or an endianness, or how a
//! variant index is spelled, would move every recorded row of every crate that
//! puts a type in a snapshot, one dependency bump at a time and with nothing to
//! say so.
//!
//! So each row here is a value and the bytes it is written down as. The rows
//! cover one of everything the `serde` data model offers that this workspace
//! puts on a wire, because a table that only covered the shapes today's types
//! happen to use would stop covering the format the day somebody adds a `char`.
//!
//! If a change here is genuinely wanted, it is a new version of the format: bump
//! the crate's major version and reissue every capture recorded under the old
//! one. Regenerating these literals to make a red test
//! go green is never the right move -- the red test *is* the notification that
//! every capture in the workspace has stopped meaning what it meant.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use std::collections::BTreeMap;

use corvid_wire::golden::{Row, check};
use serde::{Deserialize, Serialize};

/// The unsigned integers, each at its width and at a value whose bytes are not
/// all alike, so that an endianness change moves every row.
const GOLDEN_UNSIGNED: &[Row<'_>] = &[
    ("0u8", "00"),
    ("1u8", "01"),
    ("u8::MAX", "ff"),
    ("1u16", "01"),
    ("0x1234u16", "fb3412"),
    ("u16::MAX", "fbffff"),
    ("1u32", "01"),
    ("0x1234_5678u32", "fc78563412"),
    ("u32::MAX", "fcffffffff"),
    ("1u64", "01"),
    ("0x1234_5678_9abc_def0u64", "fdf0debc9a78563412"),
    ("u64::MAX", "fdffffffffffffffff"),
];

/// The signed ones, which are zigzagged before they are written: a value is
/// doubled and a negative one is folded onto the odd numbers, so `-1` is `01`
/// and `-2` is `03` and a small negative costs one byte rather than eight. The
/// negative rows are what pins that. `i8` and the two extremes are the rows that
/// show the edges -- a single byte is never zigzagged, and `i64::MIN` folds onto
/// `u64::MAX` and takes the widest marker there is.
const GOLDEN_SIGNED: &[Row<'_>] = &[
    ("-1i8", "ff"),
    ("i8::MIN", "80"),
    ("-1i16", "01"),
    ("-2i32", "03"),
    ("-2i64", "03"),
    ("i64::MIN", "fdffffffffffffffff"),
];

/// The 128-bit integers, which are the only values that reach the widest
/// marker.
///
/// `fe` is part of the format the README states, and no other row in this file
/// produces one, so without these the widest branch of the encoding is described
/// and not frozen -- an upgrade that spelled it differently, or that wrote the
/// sixteen bytes in the other order, would move nothing here.
///
/// The `1u128 << 64` row is the one that pins the order, because its bytes are
/// not a palindrome and its high half is where a big-endian writer would put the
/// `01`. The two extremes are one byte string apiece and the same one, which
/// `two_shapes_this_format_writes_alike` says out loud.
const GOLDEN_WIDE: &[Row<'_>] = &[
    ("1u128", "01"),
    ("251u128", "fbfb00"),
    ("1u128 << 64", "fe 00000000000000000100000000000000"),
    ("u128::MAX", "fe ffffffffffffffffffffffffffffffff"),
    ("-1i128", "01"),
    ("i128::MAX", "fe feffffffffffffffffffffffffffffff"),
    ("i128::MIN", "fe ffffffffffffffffffffffffffffffff"),
];

/// The floats, which are the one thing here that is *not* a varint.
///
/// A configuration chosen for its variable-length integers invites the
/// assumption that everything shrinks, and a float does not: it is its declared
/// width of IEEE-754 bytes, little-endian, whatever it holds. So `0.0f32` costs
/// four bytes where `0u32` costs one, and an `f64` costs eight where a small
/// `u64` costs one. Several crates in this workspace put an `f32` in a snapshot,
/// which makes this the group most likely to be assumed rather than read.
///
/// The fractional rows are what pin the byte order, since the zeroes and the
/// signed pair differ only in their last byte and would survive a reversal
/// looking almost right.
const GOLDEN_FLOATS: &[Row<'_>] = &[
    ("0.0f32", "00000000"),
    ("1.0f32", "0000803f"),
    ("-1.0f32", "000080bf"),
    ("0.1f32", "cdcccc3d"),
    ("0.0f64", "0000000000000000"),
    ("1.0f64", "000000000000f03f"),
    ("-1.0f64", "000000000000f0bf"),
    ("0.1f64", "9a9999999999b93f"),
];

/// The scalars that are not integers.
///
/// `char` is here even though nothing in the simulation ring carries one,
/// because the day something does it should inherit a frozen encoding rather
/// than mint one.
const GOLDEN_SCALARS: &[Row<'_>] = &[
    ("false", "00"),
    ("true", "01"),
    ("'a'", "61"),
    ("'\u{1f426}'", "f09f90a6"),
    ("()", ""),
];

/// The variable-length values, every one of which is a count and then its
/// contents.
///
/// The count is a varint like any other number, so a list of fewer than 251
/// elements pays one byte for it. That is where most of what this configuration
/// saves comes from, because the saving is per *list* rather than per element.
///
/// The empty rows are the ones that pin the count's existence: without a length
/// prefix an empty string and an empty list would both be nothing at all, and a
/// struct holding two of them could not tell which was which.
const GOLDEN_LENGTHS: &[Row<'_>] = &[
    ("\"\"", "00"),
    ("\"crow\"", "0463726f77"),
    ("\"corvid \u{1f426}\"", "0b 636f7276696420 f09f90a6"),
    ("Vec::<u8>::new()", "00"),
    ("vec![1u8, 2, 3]", "03010203"),
    ("vec![1u16, 2]", "020102"),
];

/// A count that has outgrown its one byte.
///
/// Every length row above counts to four, which leaves the marked half of the
/// count -- the half the README's table is mostly about -- recorded nowhere, even
/// though it is the varint that every container in every capture writes. 250 and
/// 251 are the two sides of the boundary, so a change to where the marker starts
/// moves one of them.
///
/// The elements are `()` and write nothing, which is what keeps these literals
/// short enough to read as counts. A container whose elements do write bytes is
/// checked in the test body, where 303 bytes can be said as a length instead.
const GOLDEN_COUNTS: &[Row<'_>] = &[
    ("vec![(); 250]", "fa"),
    ("vec![(); 251]", "fbfb00"),
    ("vec![(); 300]", "fb2c01"),
];

/// A fixed-size array, which is the one container that writes *no* count.
///
/// Its length is in its type, so `serde` offers it as a tuple. That is worth a
/// row of its own because a position in this workspace is a `[T; 3]`: the pair
/// below holds the same three numbers as a `Vec` and as an array, and the array
/// is the shorter by exactly one length prefix -- one byte here, because three is
/// a small count, and never more than nine.
const GOLDEN_ARRAYS: &[Row<'_>] = &[("[1u16, 2, 3]", "010203"), ("vec![1u16, 2, 3]", "03010203")];

/// A map, which is a count and then a key and a value for each entry.
///
/// `BTreeMap` and not `HashMap`, because the wire has an order and a hash map
/// has none to give: a recorded row of an unordered map passes on the build that
/// recorded it and is a coin toss everywhere else. The numbered fixture is built
/// out of order and encodes in key order, which is the property the row is
/// actually freezing.
///
/// The string-keyed row is here because a key is an ordinary value carrying its
/// own count, so an entry has no fixed size and a reader cannot step over one
/// without reading it.
const GOLDEN_MAPS: &[Row<'_>] = &[
    ("{}", "00"),
    ("{1: 10, 2: 20, 3: 30}", "03 010a 0214 031e"),
    ("{\"a\": 1, \"b\": 2}", "02 016101 016202"),
];

/// `Option`, which is a tag byte and then the payload if there is one.
///
/// The `Some(0)` row is the one worth having: a tag that was only written when
/// the payload was non-zero would encode `Some(0)` and `None` alike, and every
/// comparison between two present options would still pass.
const GOLDEN_OPTIONS: &[Row<'_>] = &[
    ("None::<u16>", "00"),
    ("Some(0u16)", "0100"),
    ("Some(1u16)", "0101"),
    ("Some(None::<u16>)", "0100"),
];

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

/// Nesting, because a format that flattened it would let two shapes collide.
///
/// The first two rows are the pair that pins it: one list of two elements and
/// two lists of one element each hold the same numbers in the same order, and
/// they are different byte strings only because each list writes its own count.
const GOLDEN_NESTING: &[Row<'_>] = &[
    ("vec![vec![1u8, 2]]", "01020102"),
    ("vec![vec![1u8], vec![2u8]]", "0201010102"),
    ("vec![Some(1u16), None]", "02010100"),
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
fn the_unsigned_integers_encode_as_they_were_recorded() {
    check("u8", &GOLDEN_UNSIGNED[..3], &[0_u8, 1, u8::MAX]).unwrap();
    check("u16", &GOLDEN_UNSIGNED[3..6], &[1_u16, 0x1234, u16::MAX]).unwrap();
    check(
        "u32",
        &GOLDEN_UNSIGNED[6..9],
        &[1_u32, 0x1234_5678, u32::MAX],
    )
    .unwrap();
    check(
        "u64",
        &GOLDEN_UNSIGNED[9..],
        &[1_u64, 0x1234_5678_9abc_def0, u64::MAX],
    )
    .unwrap();
}

#[test]
fn the_signed_integers_encode_as_they_were_recorded() {
    check("i8", &GOLDEN_SIGNED[..2], &[-1_i8, i8::MIN]).unwrap();
    check("i16", &GOLDEN_SIGNED[2..3], &[-1_i16]).unwrap();
    check("i32", &GOLDEN_SIGNED[3..4], &[-2_i32]).unwrap();
    check("i64", &GOLDEN_SIGNED[4..], &[-2_i64, i64::MIN]).unwrap();
}

#[test]
fn the_128_bit_integers_encode_as_they_were_recorded() {
    check(
        "u128",
        &GOLDEN_WIDE[..4],
        &[1_u128, 251, 1_u128 << 64, u128::MAX],
    )
    .unwrap();
    check("i128", &GOLDEN_WIDE[4..], &[-1_i128, i128::MAX, i128::MIN]).unwrap();
}

#[test]
fn a_float_keeps_its_declared_width_where_an_integer_does_not() {
    check("f32", &GOLDEN_FLOATS[..4], &[0.0_f32, 1.0, -1.0, 0.1]).unwrap();
    check("f64", &GOLDEN_FLOATS[4..], &[0.0_f64, 1.0, -1.0, 0.1]).unwrap();

    // The rule the rows above are an instance of, said against the varint it is
    // the exception to: the same zero costs four bytes as an `f32` and one as a
    // `u32`, so a field that changed between the two changes the size of every
    // capture holding it.
    assert_eq!(corvid_wire::encode(&0.0_f32).unwrap().len(), 4);
    assert_eq!(corvid_wire::encode(&0.0_f64).unwrap().len(), 8);
    assert_eq!(corvid_wire::encode(&0_u32).unwrap().len(), 1);
}

#[test]
fn the_other_scalars_encode_as_they_were_recorded() {
    check("bool", &GOLDEN_SCALARS[..2], &[false, true]).unwrap();
    check("char", &GOLDEN_SCALARS[2..4], &['a', '\u{1f426}']).unwrap();
    check("unit", &GOLDEN_SCALARS[4..], &[()]).unwrap();
}

#[test]
fn every_length_is_a_count_before_its_contents() {
    check(
        "str",
        &GOLDEN_LENGTHS[..3],
        &[
            String::new(),
            "crow".to_owned(),
            "corvid \u{1f426}".to_owned(),
        ],
    )
    .unwrap();
    check(
        "Vec<u8>",
        &GOLDEN_LENGTHS[3..5],
        &[Vec::new(), vec![1_u8, 2, 3]],
    )
    .unwrap();
    check("Vec<u16>", &GOLDEN_LENGTHS[5..], &[vec![1_u16, 2]]).unwrap();
}

#[test]
fn a_count_past_250_takes_a_marker_like_any_other_number() {
    check(
        "Vec<()>",
        GOLDEN_COUNTS,
        &[vec![(); 250], vec![(); 251], vec![(); 300]],
    )
    .unwrap();

    // The same count in front of elements that do write bytes, because the rows
    // above are counts with nothing after them and the thing worth knowing is
    // that the marker sits in front of the contents rather than replacing them.
    let long = corvid_wire::encode(&vec![0_u8; 300]).unwrap();
    assert_eq!(long[..3], [0xfb, 0x2c, 0x01]);
    assert_eq!(long.len(), 303);
}

#[test]
fn a_map_is_a_count_and_then_its_entries_in_key_order() {
    let numbered: BTreeMap<u16, u16> = [(2, 20), (1, 10), (3, 30)].into_iter().collect();
    let named: BTreeMap<String, u16> = [("b".to_owned(), 2), ("a".to_owned(), 1)]
        .into_iter()
        .collect();

    check(
        "BTreeMap<u16, u16>",
        &GOLDEN_MAPS[..2],
        &[BTreeMap::new(), numbered],
    )
    .unwrap();
    check("BTreeMap<String, u16>", &GOLDEN_MAPS[2..], &[named]).unwrap();
}

#[test]
fn a_fixed_size_array_writes_no_count_at_all() {
    check("[u16; 3]", &GOLDEN_ARRAYS[..1], &[[1_u16, 2, 3]]).unwrap();
    check("Vec<u16>", &GOLDEN_ARRAYS[1..], &[vec![1_u16, 2, 3]]).unwrap();

    // The count that separates them, said as a difference rather than as two
    // lengths, because it is the difference that a `[T; 3]` turned into a
    // `Vec<T>` would cost every position in a snapshot.
    let array = corvid_wire::encode(&[1_u16, 2, 3]).unwrap();
    let list = corvid_wire::encode(&vec![1_u16, 2, 3]).unwrap();
    assert_eq!(list.len(), array.len() + 1);
    assert_eq!(list[1..], array[..]);
}

#[test]
fn an_option_is_a_tag_and_then_its_payload() {
    check(
        "Option<u16>",
        &GOLDEN_OPTIONS[..3],
        &[None, Some(0_u16), Some(1)],
    )
    .unwrap();
    check(
        "Option<Option<u16>>",
        &GOLDEN_OPTIONS[3..],
        &[Some(None::<u16>)],
    )
    .unwrap();
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
fn nesting_survives_because_every_container_counts_itself() {
    check(
        "Vec<Vec<u8>>",
        &GOLDEN_NESTING[..2],
        &[vec![vec![1_u8, 2]], vec![vec![1_u8], vec![2_u8]]],
    )
    .unwrap();
    check(
        "Vec<Option<u16>>",
        &GOLDEN_NESTING[2..],
        &[vec![Some(1_u16), None]],
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
