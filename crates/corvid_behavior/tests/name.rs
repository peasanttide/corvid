//! The two bounded names this crate declares, at the bounds it declares them
//! with.
//!
//! What a bounded name *does* -- refuse, sort, print, encode as its text -- is
//! `corvid_name`'s to test, and is tested there over capacities of its own.
//! What is this crate's is the two numbers: sixty-four bytes of presence line
//! and two hundred and fifty-six of link are wire-format decisions, and a build
//! that quietly changed one would be a build whose saves the previous one
//! cannot read.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_behavior::{PresenceText, Url};

#[test]
fn the_declared_bounds_are_the_ones_a_save_was_written_against() {
    assert_eq!(PresenceText::CAPACITY, 64);
    assert_eq!(Url::CAPACITY, 256);

    // Stated as the boundary rather than as the number alone, so a widened
    // capacity fails here instead of passing a constant nobody reads.
    assert!(PresenceText::new(&"x".repeat(64)).is_ok());
    assert!(PresenceText::new(&"x".repeat(65)).is_err());
    assert!(Url::new(&"x".repeat(256)).is_ok());
    assert!(Url::new(&"x".repeat(257)).is_err());
}

/// The encoding, which exists only in a build that asked for one.
#[cfg(feature = "serde")]
#[test]
fn a_presence_line_reads_as_a_line_and_not_as_its_padding() {
    let line = PresenceText::new("defending the cellar").unwrap();
    assert_eq!(line.as_str(), "defending the cellar");

    // A varint carries no field names, so this is the one place a renamed or
    // re-shaped name would show up: the text has to be the encoding.
    let text = serde_json::to_string(&line).expect("a presence line serializes");
    assert_eq!(text, r#""defending the cellar""#);
    assert_eq!(
        serde_json::from_str::<PresenceText>(&text).expect("and deserializes"),
        line,
    );

    // The bound is re-checked on the way in, because a file is not a `&str`
    // this program built.
    assert!(serde_json::from_str::<PresenceText>(&format!(r#""{}""#, "x".repeat(65))).is_err());
}
