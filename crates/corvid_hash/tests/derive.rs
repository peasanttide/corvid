//! What `#[derive(Hash)]` turns a type into, under this crate's hasher.
//!
//! The derive is `core`'s, so what is asserted here is not that it exists but
//! that the three properties a wire format needs of it hold: a struct's fields
//! are absorbed in declaration order, an enum absorbs its variant index before
//! its payload, and a permutation of a struct's fields is a different value.
//!
//! Declaration order being the encoding is the sharp edge. Exchanging two
//! same-typed fields compiles, changes every digest of that type, and is a
//! desync rather than a test failure unless something freezes the numbers —
//! which is what `tests/golden.rs` and the golden tables across the workspace
//! are for.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use core::hash::{Hash, Hasher as _};
use std::marker::PhantomData;

use corvid_hash::{Digest, Hasher, digest};

#[derive(Hash)]
struct Named {
    a: u32,
    b: i64,
}

#[derive(Hash)]
struct Tuple(u8, u8);

#[derive(Hash)]
struct Unit;

#[derive(Hash)]
enum Choice {
    Nothing,
    One(u32),
    Two { x: u16, y: u16 },
}

#[test]
fn field_order_is_the_encoding() {
    // The two fields hold each other's values, so nothing but the order they
    // are absorbed in tells these apart.
    assert_ne!(digest(&Named { a: 1, b: 2 }), digest(&Named { a: 2, b: 1 }));
}

#[test]
fn a_derived_struct_absorbs_its_fields_at_their_declared_widths() {
    // Spelled out: a `u32` field absorbs four bytes and an `i64` field eight,
    // in declaration order and with nothing in between.
    let mut hasher = Hasher::new();
    hasher.write_u32(1);
    hasher.write_i64(2);
    assert_eq!(hasher.digest(), digest(&Named { a: 1, b: 2 }));
}

#[test]
fn a_tuple_struct_absorbs_its_fields_in_order() {
    assert_ne!(digest(&Tuple(1, 2)), digest(&Tuple(2, 1)));
}

#[test]
fn variants_are_discriminated() {
    assert_ne!(digest(&Choice::Nothing), digest(&Choice::One(0)));
    assert_ne!(digest(&Choice::One(0)), digest(&Choice::Two { x: 0, y: 0 }));
}

#[test]
fn a_variant_index_precedes_its_payload() {
    // The discriminant is absorbed as an `isize`, which this hasher widens to
    // sixty-four bits on every target. The second variant's payload is one
    // `u32`, so the whole of `Choice::One(0)` is that index and that word.
    let mut hasher = Hasher::new();
    hasher.write_isize(1);
    hasher.write_u32(0);
    assert_eq!(hasher.digest(), digest(&Choice::One(0)));
}

#[test]
fn a_variant_payload_is_absorbed() {
    assert_ne!(digest(&Choice::One(0)), digest(&Choice::One(1)));
    assert_ne!(
        digest(&Choice::Two { x: 1, y: 0 }),
        digest(&Choice::Two { x: 0, y: 1 })
    );
}

#[test]
fn a_unit_struct_still_hashes() {
    // It absorbs no word, so its digest is the empty one — which is not zero.
    assert_ne!(digest(&Unit), Digest::ZERO);
    assert_eq!(digest(&Unit), digest(&()));
}

#[test]
fn generic_parameters_gain_a_bound() {
    #[derive(Hash)]
    struct Wrapper<T> {
        inner: T,
    }

    assert_ne!(
        digest(&Wrapper { inner: 1u32 }),
        digest(&Wrapper { inner: 2u32 })
    );
}

#[test]
fn a_lifetime_is_not_given_a_bound() {
    #[derive(Hash)]
    struct Borrowed<'a> {
        name: &'a str,
    }

    assert_ne!(
        digest(&Borrowed { name: "a" }),
        digest(&Borrowed { name: "b" })
    );
}

#[test]
fn a_const_parameter_is_not_given_a_bound() {
    #[derive(Hash)]
    struct Fixed<const N: usize> {
        items: [u8; N],
    }

    assert_ne!(
        digest(&Fixed { items: [0u8; 4] }),
        digest(&Fixed { items: [1u8; 4] })
    );
}

/// The typed-identifier pattern, which simulation state is made of: a raw
/// index, and a marker saying what it indexes so two kinds of index cannot be
/// swapped by accident.
#[derive(Hash)]
struct Id<T> {
    raw: u32,
    marker: PhantomData<T>,
}

impl<T> Id<T> {
    const fn new(raw: u32) -> Self {
        Self {
            raw,
            marker: PhantomData,
        }
    }
}

/// What an `Id` can point at. A marker type absorbs nothing, so deriving on it
/// costs no word — it is only there to satisfy the bound the derive writes for
/// every type parameter.
#[derive(Hash)]
struct Ship;

/// A second one, so the marker can be shown to be a compile-time distinction
/// rather than a wire-format one.
#[derive(Hash)]
struct Station;

#[test]
fn a_typed_identifier_derives() {
    assert_ne!(digest(&Id::<Ship>::new(1)), digest(&Id::<Ship>::new(2)));
}

#[test]
fn a_marker_field_costs_no_word() {
    // The whole point of the pattern: the type parameter is checked by the
    // compiler and never paid for on the wire, so an `Id<Ship>` is its `raw`.
    assert_eq!(digest(&Id::<Ship>::new(7)), digest(&7u32));
    assert_eq!(digest(&Id::<Ship>::new(7)), digest(&Id::<Station>::new(7)));
}

#[test]
fn a_derived_type_nests_inside_another() {
    #[derive(Hash)]
    struct Outer {
        first: Choice,
        second: Tuple,
    }

    assert_ne!(
        digest(&Outer {
            first: Choice::Nothing,
            second: Tuple(0, 0)
        }),
        digest(&Outer {
            first: Choice::One(0),
            second: Tuple(0, 0)
        })
    );
}

/// Nothing is imported in here, on purpose. `Hash` is in the prelude and the
/// hasher is named by its full path, so a module — or a crate, or a `no_std`
/// target — that has never named this crate's traits still hashes through it.
mod with_nothing_in_scope {
    #[derive(Hash)]
    struct Hidden {
        value: u32,
    }

    #[derive(Hash)]
    enum Sealed {
        Empty,
        Full(Hidden),
    }

    #[test]
    fn a_derive_needs_no_imports() {
        assert_ne!(
            corvid_hash::digest(&Sealed::Empty),
            corvid_hash::digest(&Sealed::Full(Hidden { value: 0 }))
        );
    }
}
