//! What a manifest looks like written down, which is a record a person edits.

#![cfg(feature = "serde")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

use corvid_asset::{Manifest, PackId};

fn id(text: &str) -> PackId {
    PackId::new(text).expect("the identifiers in these tests are short")
}

/// A manifest writes as named fields, and its identifiers as their text.
///
/// The identifier is the claim worth pinning: a `PackId` is thirty-two bytes of
/// storage and one word of value, and a pack file that listed thirty-two
/// numbers would be unreadable and would break the day the capacity grew.
#[test]
fn a_manifest_writes_as_named_fields_and_its_identifiers_as_text() {
    let manifest = Manifest::new(id("riverside"), "Riverside", 3).requiring(id("terminus"));

    assert_eq!(
        serde_json::to_string(&manifest).unwrap(),
        r#"{"id":"riverside","name":"Riverside","version":3,"requires":["terminus"]}"#,
    );
}

/// A pack that requires nothing may leave the field out.
///
/// The base game is that pack, and a manifest that had to state an empty list
/// to say "nothing" would be a line every author copies without reading.
#[test]
fn requires_may_be_omitted() {
    let read: Manifest =
        serde_json::from_str(r#"{"id":"terminus","name":"Terminus","version":1}"#).unwrap();

    assert_eq!(read, Manifest::new(id("terminus"), "Terminus", 1));
    assert!(read.requires.is_empty());
}

/// An identifier too long for the type is refused when it is read, not cut.
///
/// Two packs whose identifiers were cut to the same thirty-two bytes would
/// override each other's files in silence, so a file is not trusted to have
/// been written by this program.
#[test]
fn an_identifier_that_does_not_fit_is_refused_when_it_is_read() {
    let too_long = "a".repeat(33);
    let record = format!(r#"{{"id":"{too_long}","name":"Long","version":1}}"#);

    assert!(serde_json::from_str::<Manifest>(&record).is_err());
}
