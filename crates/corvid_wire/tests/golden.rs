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

use corvid_wire::golden::{Row, check};

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
