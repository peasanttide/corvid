//! The two changes a byte table cannot see, and the reason a JSON table sits
//! beside it.
//!
//! `tests/visible.rs` is the case for this encoding: reordering two fields of
//! different types, renumbering a variant and adding a field each move the
//! bytes, and a widened integer does not — the digest is what catches that one.
//! This file is the other side of the same statement, and it exists so that the
//! comparison table in the README is a measurement rather than a claim. A format
//! that carries no names cannot see a change that is only in the names.
//!
//! Both cases below are narrow. Neither is a reason to prefer a self-describing
//! encoding for a capture — a capture is sent compactly and it has to be — but
//! both are reasons for a crate whose types go into a snapshot to keep a table
//! of its JSON beside its table of bytes, which is the convention this
//! workspace follows. The two tables see different halves of one encoding, and a
//! change that moves neither has not moved it.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_wire::encode;

use corvid_wire::golden::hex;
use serde::Serialize;

#[derive(Serialize)]
struct Xy {
    x: u32,
    y: u32,
}

/// The same two fields, same types, declared the other way round.
///
/// Unlike the reordering in `tests/visible.rs`, both fields here are `u32`, so
/// every use site that names them still compiles and every one that builds the
/// struct positionally is silently building a different value.
#[derive(Serialize)]
struct Yx {
    y: u32,
    x: u32,
}

/// A field appended that writes nothing at all.
///
/// A `()` is the smallest example. A unit struct, an empty tuple and a
/// zero-length array all behave the same way, and a `PhantomData` marker
/// arriving in a refactor is how this reaches a real type.
#[derive(Serialize)]
struct Marked {
    x: u32,
    marker: (),
    y: u32,
}

#[test]
fn two_fields_of_the_same_type_that_hold_the_same_value_can_swap_unseen() {
    // With different values in them the swap is visible, and that is worth
    // pinning first so the blind spot is not read as wider than it is.
    assert_ne!(
        hex(&encode(&Xy { x: 1, y: 2 }).unwrap()),
        hex(&encode(&Yx { y: 2, x: 1 }).unwrap()),
    );

    // With the same value in both, there is nothing left for the bytes to
    // differ by, because the names were never written down. A row recorded from
    // a fixture like this one is the same row after the swap — so a fixture
    // where two same-typed fields hold the same number is a fixture that has
    // stopped covering their order.
    assert_eq!(
        hex(&encode(&Xy { x: 1, y: 1 }).unwrap()),
        hex(&encode(&Yx { y: 1, x: 1 }).unwrap()),
    );
    assert_eq!(hex(&encode(&Xy { x: 1, y: 1 }).unwrap()), "0101");

    // And the column the README's table gives to a self-describing format. It
    // writes the names, so it sees the swap whatever the values are — which is
    // the one thing JSON does that this encoding cannot.
    assert_eq!(
        serde_json::to_string(&Xy { x: 1, y: 1 }).unwrap(),
        r#"{"x":1,"y":1}"#,
    );
    assert_eq!(
        serde_json::to_string(&Yx { y: 1, x: 1 }).unwrap(),
        r#"{"y":1,"x":1}"#,
    );
}

#[test]
fn a_field_that_encodes_to_nothing_is_added_unseen() {
    // Adding a field moves the bytes for any field that writes bytes, which is
    // the claim `tests/visible.rs` makes and every field a snapshot actually
    // holds. A field that writes none is the exception, and it is exactly the
    // shape a marker has.
    assert_eq!(
        hex(&encode(&Xy { x: 1, y: 1 }).unwrap()),
        hex(&encode(&Marked {
            x: 1,
            marker: (),
            y: 1,
        })
        .unwrap()),
    );

    // JSON writes a `null` for it, so the same addition is one row of a JSON
    // table going red.
    assert_eq!(
        serde_json::to_string(&Marked {
            x: 1,
            marker: (),
            y: 1,
        })
        .unwrap(),
        r#"{"x":1,"marker":null,"y":1}"#,
    );
}
