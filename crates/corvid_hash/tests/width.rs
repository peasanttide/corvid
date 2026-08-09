//! A mark has to be the same number on a 32-bit target and a 64-bit one.
//!
//! `wasm32` is a peer platform here rather than a curiosity: a browser client
//! and a native server exchange marks every tick, and the one place a value's
//! width is the *target's* rather than the type's is `usize` and `isize`. The
//! default methods on [`core::hash::Hasher`] forward `write_usize` at the
//! pointer width, so a `Vec`'s length prefix would be four bytes in the browser
//! and eight on the server and the two peers would desync on the first tick.
//!
//! This crate's hasher absorbs both as sixty-four bits on every target. The
//! assertions below are what says so, and they are written so that they would
//! fail on a target where they stopped being true rather than only on this one.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use core::hash::Hasher as _;

use corvid_hash::{Digest, Hasher, digest};

/// The digest of one `write_usize` of `value`.
fn pointer_sized(value: usize) -> Digest {
    let mut hasher = Hasher::new();
    hasher.write_usize(value);
    hasher.digest()
}

/// The digest of one `write_u64` of `value`.
fn sixty_four(value: u64) -> Digest {
    let mut hasher = Hasher::new();
    hasher.write_u64(value);
    hasher.digest()
}

#[test]
fn write_usize_and_write_u64_agree() {
    // Every value a 32-bit target can hold, and the boundary either side of the
    // width it would truncate at.
    for value in [
        0usize,
        1,
        7,
        0xffff,
        u32::MAX as usize,
        u32::MAX as usize - 1,
    ] {
        assert_eq!(
            pointer_sized(value),
            sixty_four(value as u64),
            "usize {value}"
        );
    }
}

#[test]
fn write_isize_is_sign_extended_to_sixty_four_bits() {
    // Sign extension rather than zero extension, because the alternative
    // collides `-1isize` with `usize::MAX` on a 32-bit target and with nothing
    // at all on a 64-bit one.
    for value in [0isize, 1, -1, i32::MAX as isize, i32::MIN as isize] {
        let mut narrow = Hasher::new();
        narrow.write_isize(value);
        let mut wide = Hasher::new();
        wide.write_i64(value as i64);
        assert_eq!(narrow.digest(), wide.digest(), "isize {value}");
    }
}

#[test]
fn a_vec_of_words_does_not_depend_on_the_pointer_width() {
    // `Hash for [T]` emits the count through `write_usize` and then the whole
    // slice as raw bytes, which for a `u64` on a little-endian target is the
    // same sequence `write_u64` would have produced element by element. Reading
    // this as "each element at its own width" is the trap the next test exists
    // to close.
    let elements = vec![1u64, 2, 3];

    let mut by_hand = Hasher::new();
    by_hand.write_u64(elements.len() as u64);
    for &element in &elements {
        by_hand.write_u64(element);
    }

    assert_eq!(by_hand.digest(), digest(&elements));
}

#[test]
fn the_length_prefix_is_what_separates_a_split_from_a_join() {
    // The prefix is not decoration. Without it a pair of lists and their
    // concatenation absorb the same words in the same order, which is a
    // divergence a peer could construct rather than only stumble into.
    assert_ne!(
        digest(&(vec![1u64], vec![2u64])),
        digest(&(vec![1u64, 2], Vec::<u64>::new()))
    );
}

/// The one place a pointer-sized value is *not* pinned to sixty-four bits.
///
/// `core`'s `hash_slice` specialisation covers `usize` and `isize` alongside the
/// fixed-width integers, and what it hands to `write` is `size_of_val`'s bytes --
/// four per element on `wasm32` and eight here. So the very desync
/// [`core::hash::Hasher::write_usize`] is overridden to prevent comes straight
/// back through the *elements* of a `Vec<usize>`, and no `Hasher` can intercept
/// it. Every other test in this file says the agreement holds; this one says
/// where it stops, which is the more useful half to have written down.
///
/// The assertions are against `size_of::<usize>()` rather than against eight, so
/// they state the dependence instead of hiding behind a 64-bit host: this test
/// passes on either target and means something different on each. There is no
/// fix in this crate -- the answer is that hashed state names a fixed-width
/// integer type -- so what the test defends is that the hazard stays documented.
#[test]
fn a_slice_of_pointer_sized_integers_is_not_pinned_to_sixty_four_bits() {
    let elements = [1usize, 2];

    let mut packed = Vec::new();
    for element in elements {
        packed.extend_from_slice(&element.to_le_bytes());
    }
    assert_eq!(packed.len(), 2 * size_of::<usize>());

    let mut raw = Hasher::new();
    raw.write_usize(elements.len());
    raw.write(&packed);
    assert_eq!(
        digest(&elements[..]),
        raw.digest(),
        "the elements go through `write` as raw pointer-width bytes"
    );

    // And that is the sixty-four-bit encoding only where the pointer happens to
    // be sixty-four bits wide. On a 32-bit target the two part company, which is
    // the whole content of the warning.
    let mut pinned = Hasher::new();
    pinned.write_usize(elements.len());
    for element in elements {
        pinned.write_u64(element as u64);
    }
    assert_eq!(
        digest(&elements[..]) == pinned.digest(),
        size_of::<usize>() == 8,
        "a slice of `usize` matches the sixty-four-bit encoding only on a 64-bit target"
    );
}

/// A slice of a narrower integer absorbs raw bytes, not one write per element.
///
/// `core` implements `Hash::hash_slice` for every primitive integer by
/// reinterpreting the slice and calling `write` once, which no `Hasher` can
/// override. For a `u64` on a little-endian target that happens to equal the
/// element-by-element sequence; for a `u16` it does not, because four elements
/// pack into one absorbed word.
///
/// This is what confines the crate to little-endian targets, and `lib.rs`
/// refuses to build on one where it would be wrong. The test pins the behaviour
/// so the refusal cannot be deleted as unnecessary.
#[test]
fn a_slice_of_narrow_integers_absorbs_raw_bytes() {
    let mut raw = Hasher::new();
    raw.write_usize(2);
    raw.write(&[0x01, 0x00, 0x02, 0x00]);

    let mut elementwise = Hasher::new();
    elementwise.write_usize(2);
    elementwise.write_u16(1);
    elementwise.write_u16(2);

    assert_eq!(digest(&[1_u16, 2][..]), raw.digest());
    assert_ne!(digest(&[1_u16, 2][..]), elementwise.digest());
}
