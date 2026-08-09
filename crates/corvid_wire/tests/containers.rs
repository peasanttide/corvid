//! The frozen encoding of the containers. **Changing a value in this file is a
//! wire-format break**, on the same terms as `tests/golden.rs`, which holds the
//! scalars and the argument for why these tables exist at all.
//!
//! What a container adds to a scalar is the count in front of it. These rows
//! freeze where that count goes, that a fixed-size array does not carry one, and
//! that nesting survives because each container writes its own -- the property
//! that keeps one list of two from encoding as two lists of one.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use std::collections::BTreeMap;

use corvid_wire::golden::{Row, check};

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
