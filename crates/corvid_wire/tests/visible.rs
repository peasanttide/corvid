//! The four changes a wire break is made of, and which recorded table sees each.
//!
//! This is the file the choice of encoding rests on. Three of the four move the
//! bytes and are recorded here as exact literals: reordering two struct fields
//! of different types, renumbering an enum variant, and adding a field. All
//! three compile, so nothing but a recorded table can notice them.
//!
//! The fourth -- widening an integer -- does **not** move the bytes, and that is
//! the cost of a varint stated as a test rather than as a paragraph. A number
//! below 251 is one byte whatever width it was declared at, so `u16(1)` and
//! `u32(1)` are the same byte and a byte golden is blind to the change. What
//! sees it is the digest, because `corvid_hash` injects the count of bytes it
//! absorbed and a wider integer absorbs more of them -- so a peer comparing
//! digests catches what these bytes cannot. Both halves are measured below over
//! one fixture.
//!
//! One shape escapes both, and it is the last case here: trading width between
//! two fields, where one integer widens and another narrows to pay for it. The
//! varint writes the same bytes and the hasher absorbs the same words and the
//! same total count, so neither table moves. A declared schema is what is left --
//! a build that describes `"i64"` where another describes `"i128"` refuses the
//! other's captures at load. That is a description a person maintains rather
//! than a measurement, and it is the only thing standing here.
//!
//! Each case is a pair of declarations that differ by exactly one of those
//! changes, holding the same value.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_hash::digest;
use corvid_wire::encode;
use corvid_wire::golden::hex;
use serde::Serialize;

/// The value every twin below holds, so that nothing but the declaration
/// differs. Both numbers are small -- a number below 251 is where a varint hides
/// a width, which is the subject of half this file -- and they are different from
/// each other, so a reordering has somewhere to show.
const FIRST: u16 = 1;
const SECOND: u32 = 2;

#[derive(Hash, Serialize)]
struct Base {
    first: u16,
    second: u32,
}

/// The same two fields the other way round. Their types differ, so this
/// compiles wherever the original did, which is what makes it the reordering
/// worth testing: exchanging two fields of the *same* type is a change the
/// compiler cannot see either, and neither can any encoding, because the bytes
/// are the same bytes in the same places.
#[derive(Hash, Serialize)]
struct Reordered {
    second: u32,
    first: u16,
}

/// The first field widened, holding the number it held before.
#[derive(Hash, Serialize)]
struct Widened {
    first: u32,
    second: u32,
}

/// A field appended, at its default.
#[derive(Hash, Serialize)]
struct Added {
    first: u16,
    second: u32,
    third: u8,
}

/// The same two fields under a different name, holding the same values. The one
/// change in this file that neither the bytes nor the digest can see, because
/// neither carries a name.
#[derive(Hash, Serialize)]
struct Renamed {
    across: u16,
    second: u32,
}

#[derive(Hash, Serialize)]
enum Order {
    First,
    Second,
}

/// The same two variants, declared the other way round. Every use site still
/// compiles: a variant is named, not numbered, and the number is only ever
/// assigned by the derive.
#[derive(Hash, Serialize)]
enum Renumbered {
    Second,
    First,
}

/// The one value every test below starts from, so that each comparison differs
/// by a declaration and never by a number.
const fn base() -> Base {
    Base {
        first: FIRST,
        second: SECOND,
    }
}

#[test]
fn reordering_two_fields_of_different_types_moves_the_bytes() {
    assert_eq!(hex(&encode(&base()).unwrap()), "0102");
    assert_eq!(
        hex(&encode(&Reordered {
            second: SECOND,
            first: FIRST,
        })
        .unwrap()),
        "0201",
    );
}

#[test]
fn renumbering_a_variant_moves_the_bytes() {
    // A variant index is a varint too, so a payload-free variant is one byte and
    // that byte is its number. Two declarations that number them the other way
    // round write each other's bytes.
    assert_eq!(hex(&encode(&Order::First).unwrap()), "00");
    assert_eq!(hex(&encode(&Order::Second).unwrap()), "01");
    assert_eq!(hex(&encode(&Renumbered::First).unwrap()), "01");
    assert_eq!(hex(&encode(&Renumbered::Second).unwrap()), "00");
}

#[test]
fn adding_a_field_moves_the_bytes() {
    assert_eq!(hex(&encode(&base()).unwrap()), "0102");
    assert_eq!(
        hex(&encode(&Added {
            first: FIRST,
            second: SECOND,
            third: 0,
        })
        .unwrap()),
        "010200",
    );
}

#[test]
fn widening_an_integer_does_not_move_the_bytes_and_does_move_the_digest() {
    let widened = Widened {
        first: u32::from(FIRST),
        second: SECOND,
    };

    // The byte table is blind to this, exactly. Not "differs by a length" --
    // identical, byte for byte, so no recorded row anywhere in the workspace
    // moves when a field of this shape is widened.
    assert_eq!(hex(&encode(&base()).unwrap()), "0102");
    assert_eq!(hex(&encode(&widened).unwrap()), "0102");

    // And the digest table is not. The hasher absorbs `first` as two bytes on
    // one side and four on the other, injects the total count at the end, and
    // answers differently -- which is what a peer compares every tick.
    assert_eq!(digest(&base()).to_u64(), 0x0dbe_2df1_4a0d_0c8c);
    assert_eq!(digest(&widened).to_u64(), 0x0fc6_cbb3_9747_e543);
}

/// The same two bytes and the same digest, one field widened and the other
/// narrowed to pay for it.
///
/// A plausible edit -- the identifier that ran out of room borrows from the one
/// that never needed it -- and the one shape in this file that no recorded table
/// in the workspace can see.
#[derive(Hash, Serialize)]
struct Traded {
    first: u32,
    second: u16,
}

#[test]
fn trading_width_between_two_fields_moves_nothing_a_table_records() {
    let traded = Traded {
        first: u32::from(FIRST),
        second: 2,
    };

    // The bytes, because a varint spells both numbers the same either way.
    assert_eq!(hex(&encode(&base()).unwrap()), "0102");
    assert_eq!(hex(&encode(&traded).unwrap()), "0102");

    // The digest, because the hasher absorbs `1` and `2` as the same two words
    // either way and the counts it injects -- two plus four, four plus two --
    // come to the same six.
    assert_eq!(digest(&base()).to_u64(), 0x0dbe_2df1_4a0d_0c8c);
    assert_eq!(digest(&traded).to_u64(), 0x0dbe_2df1_4a0d_0c8c);

    // So a build that made this edit reads the other build's capture to the end
    // and reports success, and `decode`'s trailing-byte check cannot help: the
    // bytes are not merely the same length, they are the same bytes.
    let misread: (u16, u32) = corvid_wire::decode(&encode(&traded).unwrap()).unwrap();
    assert_eq!(misread, (FIRST, SECOND));

    // What is left is a declared schema, which is a description rather than a
    // measurement: the two builds have to spell these widths differently in the
    // string they hash, and then a capture from one is refused by the other at
    // load.
}

/// The rest of the README's digest row, which the tests above leave at three
/// cells out of five.
///
/// The four changes are checked against one table each in the tests above,
/// because that is the table the choice of encoding turns on. But the README
/// prints a grid, and a grid is a claim about every cell in it. These are the
/// remaining ones, so the row is measured rather than argued.
#[test]
fn the_digest_sees_every_change_these_bytes_do_and_no_name() {
    let recorded = digest(&base()).to_u64();

    // Field order: visible, because the hasher absorbs the two words in the
    // other order.
    assert_ne!(
        digest(&Reordered {
            second: SECOND,
            first: FIRST,
        })
        .to_u64(),
        recorded,
    );

    // An added field: visible, and unlike the byte table it is visible even for
    // a field that writes no bytes -- `Hash` reaches every field whatever it
    // encodes to. `tests/blind.rs` is the other half of that comparison.
    assert_ne!(
        digest(&Added {
            first: FIRST,
            second: SECOND,
            third: 0,
        })
        .to_u64(),
        recorded,
    );

    // A variant's number: visible, because the derive hashes the discriminant.
    assert_ne!(
        digest(&Order::First).to_u64(),
        digest(&Renumbered::First).to_u64(),
    );

    // A field's name: invisible, which is the one cell where the digest is as
    // blind as the bytes. Only a self-describing table sees this.
    assert_eq!(
        digest(&Renamed {
            across: FIRST,
            second: SECOND,
        })
        .to_u64(),
        recorded,
    );
}
