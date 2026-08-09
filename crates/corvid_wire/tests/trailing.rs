//! What `decode` refuses, and that it refuses each of them differently.
//!
//! A capture is a file or a packet, and neither comes with a promise about its
//! length. Three things can be wrong with one: there is not enough of it, there
//! is too much of it, and it says there is far more of it than there is. The
//! first is the decoder running out; the second is the one a decoder is under no
//! obligation to notice and this one does; the third turns out to be the first
//! one wearing a hat, and the last test here is what establishes that.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_wire::{decode, encode};

use corvid_wire::Error;
#[test]
fn an_exact_capture_reads() {
    let bytes = encode(&(1_u16, 2_u32)).unwrap();
    assert_eq!(bytes.len(), 2);
    assert_eq!(decode::<(u16, u32)>(&bytes).unwrap(), (1, 2));
}

#[test]
fn a_capture_that_grew_is_refused_rather_than_truncated() {
    let bytes = encode(&(1_u16, 2_u32)).unwrap();

    // One extra byte is the smallest version skew there is, and the value in
    // front of it still parses. Reading it and stopping would be a save file
    // from a newer build loading as a subset of itself.
    let mut grown = bytes.clone();
    grown.push(0);
    assert_eq!(
        decode::<(u16, u32)>(&grown),
        Err(Error::Trailing { used: 2, len: 3 }),
    );

    // And a whole second value on the end, which is the shape a concatenation
    // of two captures has.
    let mut twice = bytes.clone();
    twice.extend_from_slice(&bytes);
    assert_eq!(
        decode::<(u16, u32)>(&twice),
        Err(Error::Trailing { used: 2, len: 4 }),
    );
}

#[test]
fn a_capture_that_was_cut_short_is_refused_too() {
    let bytes = encode(&(1_u16, 2_u32)).unwrap();
    let short = decode::<(u16, u32)>(&bytes[..1]);
    assert!(matches!(short, Err(Error::Read(_))), "{short:?}");

    // Nothing at all is the degenerate case of the same thing, and it is worth
    // naming because an empty file is what a run that was killed mid-write
    // leaves behind.
    assert!(matches!(decode::<(u16, u32)>(&[]), Err(Error::Read(_))));
}

/// A hostile count and an honest one that was cut short are the same failure --
/// for the one shape where the slice really does settle it.
///
/// A `Vec<u32>` is read through `serde`'s sequence path, which reserves against
/// a size hint it caps, so the count below is compared against what is there
/// rather than acted on. That is the shape this test has always used, and it is
/// the reason the crate's ceiling had to be found by trying a different one:
/// `tests/hostile.rs` is the same idea over a `String`, where the count *is*
/// acted on and eighteen quintillion of them is an allocation.
#[test]
fn a_length_no_capture_could_hold_fails_exactly_as_a_short_one_does() {
    // A count is a varint, so the widest marker there is followed by eight bytes
    // of `ff` is how a peer spells eighteen quintillion elements.
    let mut hostile = vec![0xfd_u8];
    hostile.extend_from_slice(&u64::MAX.to_le_bytes());
    hostile.push(0);

    let mut honest = vec![0x02_u8];
    honest.push(0);

    let refused = decode::<Vec<u32>>(&hostile);
    assert!(matches!(refused, Err(Error::Read(_))), "{refused:?}");
    assert_eq!(refused, decode::<Vec<u32>>(&honest));

    // And an honest count is still read, which is what says the paragraph above
    // is about how the count is checked rather than about counts being ignored.
    let modest = encode(&vec![1_u32, 2, 3]).unwrap();
    assert_eq!(decode::<Vec<u32>>(&modest).unwrap(), vec![1, 2, 3]);
}

/// Whatever `encode` writes, `decode` reads -- which is what the ceiling being on
/// both paths buys.
///
/// A limit on the read path alone is the worse of the two bugs: it writes a
/// capture without complaint and refuses to read it back, so a save file is lost
/// at the moment somebody needs it. `bincode` applies a configured limit to
/// reading only, so `encode` carries the check itself. The capture below is over
/// sixty-four mebibytes -- past where such a bound usually gets set, and well
/// under this crate's -- and it survives the round trip.
#[test]
fn a_capture_larger_than_a_bound_usually_set_still_makes_the_round_trip() {
    // Large values, so that each costs its marker and its eight bytes rather
    // than the one byte a varint spends on a small number.
    let big: Vec<u64> = (0..8_519_680).map(|n| u64::MAX - n).collect();
    let bytes = encode(&big).unwrap();
    assert!(bytes.len() > 64 << 20, "{} bytes", bytes.len());
    assert!(bytes.len() < corvid_wire::CEILING, "{} bytes", bytes.len());
    assert_eq!(decode::<Vec<u64>>(&bytes).unwrap(), big);
}

#[test]
fn what_went_wrong_is_readable() {
    let mut grown = encode(&(1_u16, 2_u32)).unwrap();
    grown.push(0);

    let why = decode::<(u16, u32)>(&grown).unwrap_err().to_string();
    assert!(why.contains("2 of 3"), "{why}");
    assert!(why.contains("1 were left over"), "{why}");
}

/// A `Trailing` a caller built by hand, with the fields the wrong way round.
///
/// The variant's fields are `pub`, and `#[non_exhaustive]` stops exhaustive
/// matching rather than construction -- so `used > len` is reachable from outside
/// this crate, and the subtraction in `Display` is the one place a `Display`
/// could have panicked. It is saturating, and this is what says so.
#[test]
fn a_leftover_count_that_cannot_be_is_still_printable() {
    let impossible = Error::Trailing { used: 5, len: 2 };
    assert!(impossible.to_string().contains("5 of 2"), "{impossible}");
}
