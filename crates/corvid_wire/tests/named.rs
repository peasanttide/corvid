//! The two types that want their field names, and which half each fails in.
//!
//! This format carries no names, so a type that needs them cannot be written
//! down and read back. That much is expected. What is not obvious — and what
//! this crate's error documentation had backwards until these were run — is
//! that the two usual shapes fail at opposite ends, and a person reading a
//! failure is looking at a different error than the doc had led them to.
//!
//! The rule underneath is that `serde` splits the two halves. `#[serde(flatten)]`
//! changes what the *writer* does: it emits a map whose length is not known
//! before its contents are, and a format that writes a count first cannot start
//! one. An untagged enum changes what the *reader* does: the writer emits the
//! chosen variant's payload and nothing else, which this format is perfectly
//! happy to write, and the reader then asks the bytes what they are, which is
//! the one question they cannot answer.
//!
//! Both are findings rather than false alarms — a snapshot is sent compactly,
//! and neither of these can be — but they are found at different times, and one
//! of them produces a byte string that exists and cannot be read.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_wire::{decode, encode};

use corvid_wire::Error;
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Inner {
    a: u32,
    b: u32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Flattened {
    tick: u32,
    #[serde(flatten)]
    inner: Inner,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum Untagged {
    One(u32),
    Two { a: u32, b: u32 },
}

#[test]
fn a_flattened_field_never_reaches_a_decoder() {
    let value = Flattened {
        tick: 1,
        inner: Inner { a: 2, b: 3 },
    };

    // No bytes at all. The failure is in the encoder, which is the better half
    // to fail in: nothing was written, so there is no capture in existence that
    // somebody will try to read next year.
    let refused = encode(&value).unwrap_err();
    assert!(matches!(refused, Error::Wrote(_)), "{refused:?}");
    assert!(
        refused.to_string().contains("SequenceMustHaveLength"),
        "{refused}",
    );
}

#[test]
fn an_untagged_enum_writes_down_and_cannot_be_read_back() {
    let value = Untagged::Two { a: 7, b: 9 };

    // This half succeeds, and that is the whole hazard. Two bytes exist, they
    // are a perfectly good pair of `u32`s, and nothing about them says which
    // variant wrote them — so the *only* thing that could have told them apart
    // is the name this format does not carry.
    let bytes = encode(&value).unwrap();
    assert_eq!(bytes, [0x07, 0x09]);

    let refused = decode::<Untagged>(&bytes).unwrap_err();
    assert!(matches!(refused, Error::Read(_)), "{refused:?}");
    assert!(refused.to_string().contains("AnyNotSupported"), "{refused}");

    // And the ambiguity is real rather than a technicality the decoder is being
    // fussy about: the other variant's payload is a prefix of these bytes.
    assert_eq!(encode(&Untagged::One(7)).unwrap(), bytes[..1]);
}
