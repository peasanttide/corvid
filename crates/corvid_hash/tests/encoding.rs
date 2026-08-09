//! The encoding is what stops two different values from agreeing.
//!
//! Each test here names one way an encoding can lose information -- a missing
//! length, a missing discriminant, a sign that was never extended, a string
//! that could be mistaken for the bytes it holds -- and proves this one does
//! not.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use core::hash::{Hash, Hasher as _};
use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
};

use corvid_hash::{Digest, Hasher, digest};

#[test]
fn a_length_prefix_separates_a_split_from_a_join() {
    let split = digest(&(vec![1u8], vec![2u8]));
    let joined = digest(&(vec![1u8, 2u8], Vec::<u8>::new()));
    assert_ne!(split, joined);
}

#[test]
fn nesting_is_not_flattening() {
    let nested = digest(&vec![vec![1u8, 2], vec![3]]);
    let flat = digest(&vec![vec![1u8, 2, 3]]);
    assert_ne!(nested, flat);
}

#[test]
fn an_option_carries_a_discriminant() {
    assert_ne!(digest(&Some(0u64)), digest(&None::<u64>));
    assert_ne!(digest(&Some(Some(1u64))), digest(&Some(1u64)));
    assert_ne!(digest(&Ok::<u8, u8>(1)), digest(&Err::<u8, u8>(1)));
}

#[test]
fn a_discriminant_is_absorbed_before_its_payload() {
    // `Some` is variant one, absorbed as an `isize` and therefore as sixty-four
    // bits here, and the payload follows at its own declared width.
    let mut hasher = Hasher::new();
    hasher.write_isize(1);
    hasher.write_u64(0);
    assert_eq!(hasher.digest(), digest(&Some(0u64)));
}

#[test]
fn a_signed_integer_is_its_two_s_complement_at_its_own_width() {
    // Signedness is not in the encoding -- the width is. `-1i8` and `255u8` are
    // one byte of `0xff` each, and they agree; `-1i8` and `-1i32` are one byte
    // and four, and they do not.
    assert_eq!(digest(&-1i8), digest(&255u8));
    assert_eq!(digest(&-1i32), digest(&u32::MAX));
    assert_ne!(digest(&-1i8), digest(&-1i32));
}

#[test]
fn a_pointer_sized_integer_is_absorbed_as_sixty_four_bits() {
    // The one place a width is not the type's: `usize` and `isize` are as wide
    // as the target's pointer, and this hasher absorbs both as sixty-four bits
    // so a browser peer and a native one produce the same mark.
    let mut pointer_sized = Hasher::new();
    pointer_sized.write_usize(7);
    let mut sixty_four = Hasher::new();
    sixty_four.write_u64(7);
    assert_eq!(pointer_sized.digest(), sixty_four.digest());

    // And the signed half is sign-extended to that width rather than
    // zero-extended, which is the choice that keeps `-1isize` from colliding
    // with the largest index a target can name.
    let mut negative = Hasher::new();
    negative.write_isize(-1);
    let mut widened = Hasher::new();
    widened.write_i64(-1);
    assert_eq!(negative.digest(), widened.digest());
}

#[test]
fn an_integer_is_absorbed_at_its_declared_width() {
    // The same number at two widths is two inputs, because each absorbs as many
    // bytes as its type has. This is the property that lets an encoding be read
    // back: a field's width is part of what a peer has to agree about, and the
    // opening's schema digest is where that agreement is established.
    assert_ne!(digest(&1u8), digest(&1u64));
    assert_ne!(digest(&1i8), digest(&1i64));
    assert_ne!(digest(&0u32), digest(&0u64));

    // Spelled out, so the widths are stated rather than implied.
    let mut byte = Hasher::new();
    byte.write_u8(1);
    assert_eq!(byte.digest(), digest(&1u8));

    let mut word = Hasher::new();
    word.write_u64(1);
    assert_eq!(word.digest(), digest(&1u64));
}

/// A float at eight fractional bits, which is `I24F8`'s scale -- spelled out
/// here rather than reached for, so this file keeps depending on nothing.
#[expect(
    clippy::cast_possible_truncation,
    reason = "every value this is called with is a small constant, and what the test is about is that the conversion happens at all"
)]
fn fixed(metres: f64) -> i32 {
    (metres * 256.0) as i32
}

#[test]
fn a_float_that_was_converted_is_hashed_as_what_it_was_converted_to() {
    // A float has no `Hash`, so the only float that reaches a digest is one
    // somebody turned into an integer first, and it is the integer that is
    // hashed.
    //
    // The two zeroes are why this matters and why the conversion has to be the
    // game's own decision. They compare equal, so `Data`'s `Eq` bound is happy
    // with either, and they have different bit patterns, so a digest over the
    // bits would have called them different states. Rounded to a fixed-point
    // quantity they are one value again, which is the answer a simulation
    // wants.
    let positive = fixed(0.0);
    let negative = fixed(-0.0);
    assert_eq!(digest(&positive), digest(&negative));

    // And the conversion is not a way of hashing nothing: two floats that
    // differ by more than the fixed-point step still differ afterwards.
    let step = fixed(0.5);
    assert_ne!(digest(&positive), digest(&step));
}

#[test]
fn a_string_is_its_bytes_and_a_terminator() {
    assert_eq!(digest("crow"), digest(&String::from("crow")));
    // The owned string is the borrowed one, on an input where an implementation
    // that counted something other than bytes would part company with `str` --
    // which for ASCII alone it never would.
    assert_eq!(digest("na\u{ef}ve"), digest(&String::from("na\u{ef}ve")));
    assert_eq!(
        digest("corvid \u{1f426}"),
        digest(&String::from("corvid \u{1f426}"))
    );
    assert_ne!(digest(&("a", "bc")), digest(&("ab", "c")));
    // Content alone is enough to separate these two: one NUL is not nothing.
    assert_ne!(digest(""), digest("\0"));

    // Spelled out. A string absorbs its bytes packed eight to a word and then a
    // `0xff` byte, which is what keeps a concatenation from being mistaken for
    // a pair -- `0xff` is one of the bytes no UTF-8 sequence contains, so no
    // string's own bytes can spell the terminator.
    let mut text = Hasher::new();
    text.write(b"ab");
    text.write_u8(0xff);
    assert_eq!(text.digest(), digest("ab"));
}

#[test]
fn a_string_and_a_sequence_of_its_bytes_are_different_encodings() {
    // A `str` writes its bytes and a terminator; a slice writes its element
    // count and then the elements. Two encodings that produced the same digest
    // for the same bytes would be a hazard across the two families, and the
    // wire format documents them apart.
    assert_ne!(digest("ab"), digest(&vec![b'a', b'b']));
    assert_ne!(digest("ab"), digest(b"ab".as_slice()));
    assert_ne!(digest("a"), digest(&vec![b'a']));

    // The slice half, spelled out: an element count as sixty-four bits, then
    // the bytes.
    let mut elements = Hasher::new();
    elements.write_usize(2);
    elements.write(b"ab");
    assert_eq!(elements.digest(), digest(&vec![b'a', b'b']));
}

#[test]
fn a_marker_absorbs_nothing() {
    // The typed-identifier pattern rests on this: a `PhantomData` field costs
    // no word, so a raw index wrapped in a type is the raw index on the wire.
    struct Ship;
    assert_eq!(
        digest(&(1u32, PhantomData::<Ship>, 2u32)),
        digest(&(1u32, 2u32))
    );
    assert_eq!(digest(&PhantomData::<Ship>), digest(&()));
    // Including for an unsized parameter, which is what `PhantomData<dyn _>`
    // and `PhantomData<[T]>` are made of.
    assert_eq!(digest(&PhantomData::<[u8]>), digest(&()));
}

#[test]
fn an_array_absorbs_a_length_as_a_slice_does() {
    // An array's length is in its type and is absorbed anyway, so the two
    // spellings of the same three bytes are one input rather than two.
    let array = [1u8, 2, 3];
    assert_eq!(digest(&array), digest(array.as_slice()));
    assert_eq!(digest(array.as_slice()), digest(&vec![1u8, 2, 3]));
}

#[test]
fn ordered_collections_do_not_depend_on_insertion_order() {
    let forwards: BTreeMap<u8, u8> = (0..16).map(|i| (i, i * 2)).collect();
    let backwards: BTreeMap<u8, u8> = (0..16).rev().map(|i| (i, i * 2)).collect();
    assert_eq!(digest(&forwards), digest(&backwards));

    let set: BTreeSet<u8> = (0..16).collect();
    let reversed: BTreeSet<u8> = (0..16).rev().collect();
    assert_eq!(digest(&set), digest(&reversed));
    assert_ne!(digest(&set), digest(&forwards));
}

#[test]
fn a_pointer_digests_as_what_it_points_at() {
    let boxed: Box<u32> = Box::new(7);
    let shared = std::sync::Arc::new(7u32);
    assert_eq!(digest(&boxed), digest(&7u32));
    assert_eq!(digest(&shared), digest(&7u32));
    assert_eq!(digest(&&7u32), digest(&7u32));
}

#[test]
fn a_tuple_is_its_fields_in_order() {
    assert_ne!(digest(&(1u8, 2u8)), digest(&(2u8, 1u8)));
    assert_eq!(digest(&((), 1u8, ())), digest(&1u8));
}

#[test]
fn the_encoding_does_not_name_the_type() {
    // Nothing absorbs a type tag, so two types whose encodings coincide agree.
    // That is deliberate: a peer's marks are only meaningful once both sides
    // have compared the opening's schema digest, which is what establishes
    // that they are running the same types at all.
    assert_eq!(digest(&Digest::ZERO), digest(&0u64));
    assert_eq!(digest(&()), digest(&PhantomData::<u8>));
}

#[test]
fn digest_is_the_same_thing_as_hashing_by_hand() {
    let mut hasher = Hasher::new();
    (1u32, 2u32).hash(&mut hasher);
    assert_eq!(hasher.digest(), digest(&(1u32, 2u32)));
}

#[test]
fn a_digest_prints_as_sixteen_hex_digits() {
    let digest = Digest::from_u64(0x0123_4567_89ab_cdef);
    assert_eq!(digest.to_string(), "0123456789abcdef");
    assert_eq!(format!("{digest:?}"), "Digest(0x0123456789abcdef)");
    assert_eq!(Digest::ZERO.to_string(), "0000000000000000");
    assert_eq!(digest.to_u64(), 0x0123_4567_89ab_cdef);
}
