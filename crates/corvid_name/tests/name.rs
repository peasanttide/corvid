//! What a bounded name refuses, how it sorts, and how it prints.
//!
//! Two capacities rather than one, because half of what this crate claims is
//! that the capacity is part of the type: two names holding the same text and
//! bounded differently are different values, and a test with one capacity in it
//! cannot see that.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_hash::digest;
use corvid_name::{InvalidName, bounded_name};

bounded_name! {
    /// Sixty-four bytes, as a friends-list line is.
    Line, 64
}

bounded_name! {
    /// The same text, bounded somewhere else.
    Wide, 256
}

#[test]
fn a_name_holds_the_text_it_was_given() {
    let line = Line::new("defending the cellar").unwrap();
    assert_eq!(line.as_str(), "defending the cellar");
    assert_eq!(line.len(), 20);
    assert!(!line.is_empty());
    assert!(Line::EMPTY.is_empty());
    assert_eq!(Line::CAPACITY, 64);
}

/// The encoding, which exists only in a build that asked for one.
#[cfg(feature = "serde")]
#[test]
fn a_name_survives_the_round_trip_as_the_string_it_is() {
    let line = Line::new("defending the cellar").unwrap();
    let text = serde_json::to_string(&line).expect("a name serializes");
    assert_eq!(
        text, r#""defending the cellar""#,
        "a name should read as a name",
    );
    assert_eq!(
        serde_json::from_str::<Line>(&text).expect("and deserializes"),
        line,
    );
}

#[test]
fn a_name_that_does_not_fit_is_refused_rather_than_cut() {
    // Truncation would let two different lines answer to one value, and a save
    // written against the second would read back as the first.
    assert_eq!(
        Line::new(&"x".repeat(64)).map(|name| name.len()),
        Ok(64),
        "sixty-four bytes is the limit and must fit",
    );
    assert_eq!(
        Line::new(&"x".repeat(65)),
        Err(InvalidName::TooLong {
            len: 65,
            capacity: 64,
        }),
    );
    assert_eq!(Line::new("ab\0cd"), Err(InvalidName::InteriorNul { at: 2 }));

    assert!(Wide::new(&"x".repeat(257)).is_err());
    assert!(Wide::new(&"x".repeat(256)).is_ok());
}

/// The same refusal on the way in from a file, which is not a `&str` this
/// program built and so is the case that actually needs re-checking.
#[cfg(feature = "serde")]
#[test]
fn a_name_read_back_from_a_file_is_bounded_again() {
    assert!(serde_json::from_str::<Line>(&format!(r#""{}""#, "x".repeat(65))).is_err());
    assert!(serde_json::from_str::<Line>(&format!(r#""{}""#, "x".repeat(64))).is_ok());
}

#[test]
fn names_order_as_the_strings_they_are_and_digest_as_the_arrays_they_hold() {
    // NUL sorts below every byte a name may hold, so the array order and the
    // string order are one order -- which is what a sorted list relies on.
    let mut names = [
        Line::new("terminus").unwrap(),
        Line::new("arrival").unwrap(),
        Line::new("ab").unwrap(),
        Line::new("abc").unwrap(),
    ];
    names.sort_unstable();
    let sorted: Vec<&str> = names.iter().map(Line::as_str).collect();
    assert_eq!(sorted, ["ab", "abc", "arrival", "terminus"]);

    assert_ne!(
        digest(&Line::new("ab").unwrap()),
        digest(&Line::new("abc").unwrap()),
    );

    // The digest is over the padded array, so the capacity is in the encoding:
    // widening a name type is a wire-format change rather than a local one.
    assert_ne!(
        digest(&Line::new("ab").unwrap()),
        digest(&Wide::new("ab").unwrap()),
    );
}

#[test]
fn a_name_prints_as_itself() {
    let line = Line::new("terminus").unwrap();
    assert_eq!(line.to_string(), "terminus");
    assert_eq!(format!("{line:?}"), r#"Line("terminus")"#);

    // The padding is storage rather than value, so neither form prints it.
    assert_eq!(format!("{:?}", Line::EMPTY), r#"Line("")"#);
}
