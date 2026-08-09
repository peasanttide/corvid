//! The bounded names: what they refuse, how they sort, and how they print.
//!
//! The closed vocabulary these names sit inside is tested in `src/command.rs`,
//! and it has to be: `Command` is `#[non_exhaustive]`, so a match written out
//! here gets a fallback arm forced on it by the compiler and could never notice
//! a variant that had gone missing from a fixture. A name has nothing to be
//! exhaustive about, so these belong out here, where they exercise the API a
//! game actually sees.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_behavior::{InvalidName, PresenceText, Url};

use corvid_hash::digest;
#[test]
fn a_name_survives_the_round_trip_as_the_string_it_is() {
    let line = PresenceText::new("defending the cellar").unwrap();
    assert_eq!(line.as_str(), "defending the cellar");
    assert_eq!(line.len(), 20);
    assert!(!line.is_empty());
    assert!(PresenceText::EMPTY.is_empty());

    let text = serde_json::to_string(&line).expect("a presence line serializes");
    assert_eq!(
        text, r#""defending the cellar""#,
        "a name should read as a name",
    );
    assert_eq!(
        serde_json::from_str::<PresenceText>(&text).expect("and deserializes"),
        line,
    );
}

#[test]
fn a_name_that_does_not_fit_is_refused_rather_than_cut() {
    // Truncation would let two different lines answer to one value, and a save
    // written against the second would read back as the first.
    assert_eq!(
        PresenceText::new(&"x".repeat(64)).map(|name| name.len()),
        Ok(64),
        "sixty-four bytes is the limit and must fit",
    );
    assert_eq!(
        PresenceText::new(&"x".repeat(65)),
        Err(InvalidName::TooLong {
            len: 65,
            capacity: 64,
        }),
    );
    assert_eq!(
        PresenceText::new("ab\0cd"),
        Err(InvalidName::InteriorNul { at: 2 }),
    );

    // The same check on the way in from a file, which is not a `&str` this
    // program built.
    assert!(serde_json::from_str::<PresenceText>(&format!(r#""{}""#, "x".repeat(65))).is_err());
    assert!(serde_json::from_str::<PresenceText>(&format!(r#""{}""#, "x".repeat(64))).is_ok());
    assert!(Url::new(&"x".repeat(257)).is_err());
    assert!(Url::new(&"x".repeat(256)).is_ok());
}

#[test]
fn names_order_as_the_strings_they_are_and_digest_as_the_arrays_they_hold() {
    // NUL sorts below every byte a name may hold, so the array order and the
    // string order are one order -- which is what a sorted list relies on.
    let mut names = [
        PresenceText::new("terminus").unwrap(),
        PresenceText::new("arrival").unwrap(),
        PresenceText::new("ab").unwrap(),
        PresenceText::new("abc").unwrap(),
    ];
    names.sort_unstable();
    let sorted: Vec<&str> = names.iter().map(PresenceText::as_str).collect();
    assert_eq!(sorted, ["ab", "abc", "arrival", "terminus"]);

    assert_ne!(
        digest(&PresenceText::new("ab").unwrap()),
        digest(&PresenceText::new("abc").unwrap()),
    );

    // The digest is over the padded array, so the capacity is in the encoding:
    // widening a name type is a wire-format change, and a golden row is what
    // says so.
    assert_ne!(
        digest(&PresenceText::new("ab").unwrap()),
        digest(&Url::new("ab").unwrap()),
    );
}

#[test]
fn a_name_prints_as_itself() {
    let line = PresenceText::new("terminus").unwrap();
    assert_eq!(line.to_string(), "terminus");
    assert_eq!(format!("{line:?}"), r#"PresenceText("terminus")"#);
}
