//! Ten bytes from a peer, and what they are allowed to ask for.
//!
//! A container is sized from its count before a byte of its contents is read: a
//! `String` whose prefix says two to the thirty-sixth is a `vec![0u8; 1 << 36]`
//! and then a read. So the count has to be refused on the strength of the number
//! alone, which is what [`corvid_wire::CEILING`] is for — without it these
//! inputs reserve four gibibytes, panic in `raw_vec`, or abort the process, and
//! all three are reachable from a packet shorter than this sentence.
//!
//! Every case here is under a dozen bytes. That is the point: the cost to send
//! one is nothing, and a `Result` that aborts instead of returning is not a
//! `Result`.
//!
//! `Vec<u32>` is deliberately absent. It is the one shape that was safe all
//! along — `serde`'s sequence path caps the capacity it reserves against a size
//! hint — and it is the shape `tests/trailing.rs` had been generalising from.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_wire::{Error, decode, encode};
use serde::{Deserialize, Serialize};

/// A shape with a `String` in it, which is what a name, a level's path or a chat
/// line is, and is therefore in most things a peer sends.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Named {
    name: String,
    tick: u32,
}

/// A count written the way a peer would have to write it: the widest marker, and
/// then the number little-endian.
fn count(claimed: u64) -> Vec<u8> {
    let mut bytes = vec![0xfd_u8];
    bytes.extend_from_slice(&claimed.to_le_bytes());
    bytes
}

#[test]
fn a_string_claiming_more_than_a_capture_may_hold_is_refused_unread() {
    for claimed in [
        u64::MAX,
        1 << 36,
        u64::from(u32::MAX),
        corvid_wire::CEILING as u64 + 1,
    ] {
        let mut hostile = count(claimed);
        hostile.push(0);

        let refused = decode::<Named>(&hostile);
        assert!(
            matches!(refused, Err(Error::TooLarge { wrote: None })),
            "a claim of {claimed} was answered with {refused:?}",
        );
    }
}

#[test]
fn the_same_claim_inside_a_list_and_inside_a_struct_is_refused_too() {
    // One `String` reached through a sequence, and one reached through a struct
    // field, because the two take different paths into the decoder and only one
    // of them was ever exercised.
    let mut hostile = vec![0x01_u8];
    hostile.extend_from_slice(&count(u64::MAX));
    assert!(matches!(
        decode::<Vec<String>>(&hostile),
        Err(Error::TooLarge { wrote: None }),
    ));

    let mut bare = count(u64::MAX);
    bare.push(0);
    assert!(matches!(
        decode::<String>(&bare),
        Err(Error::TooLarge { wrote: None }),
    ));
}

#[test]
fn a_refusal_says_which_ceiling_and_that_nothing_was_read() {
    let mut hostile = count(u64::MAX);
    hostile.push(0);

    let why = decode::<String>(&hostile).unwrap_err().to_string();
    assert!(why.contains(&corvid_wire::CEILING.to_string()), "{why}");
    assert!(why.contains("before anything was read"), "{why}");
}

#[test]
fn an_honest_string_of_the_same_shape_still_reads() {
    // The refusal above is about the size claimed and not about the shape, which
    // this is what says: the identical type, written down honestly, survives.
    let honest = Named {
        name: "a peer with a name".into(),
        tick: 7,
    };
    let bytes = encode(&honest).unwrap();
    assert_eq!(decode::<Named>(&bytes).unwrap(), honest);
}

#[test]
fn a_capture_too_large_to_read_back_is_one_that_will_not_be_written() {
    // The other half of the ceiling, and the reason it is on both paths. A limit
    // that applied to reading alone would let this through and then refuse the
    // bytes it produced, which loses a save file rather than refusing one.
    let past_it = vec![0_u8; corvid_wire::CEILING + 1];

    let refused = encode(&past_it);
    assert!(
        matches!(refused, Err(Error::TooLarge { wrote: Some(len) }) if len > corvid_wire::CEILING),
        "{refused:?}",
    );
}
