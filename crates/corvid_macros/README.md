# `corvid_macros`

The declarative macros Corvid's crates share: [`id_type!`], which declares a
numbered identifier, and [`named_enum!`], which declares an enumeration whose
variants each have a name a person reads.

```rust
use corvid_macros::{id_type, named_enum};

id_type! {
    /// Which seat, in a session's roster.
    SeatId, u16, "The position in the roster."
}

// A number, with the field public, because an identifier is a number. The
// display names the type, since which kind of identifier a number is is the
// thing the newtype exists to keep straight.
let seat = SeatId(3);
assert_eq!(seat.0, 3);
assert_eq!(seat.to_string(), "SeatId(3)");

// Behind the calling crate's `serde` feature it encodes as the bare number,
// because `#[serde(transparent)]` is part of what the macro declares there.
#[cfg(feature = "serde")]
{
    let json = serde_json::to_string(&seat).expect("a u16 has a json encoding");
    assert_eq!(json, "3");
}

// It is not the other kind of identifier: `takes(seat)` would not compile.
id_type! {
    /// Which account.
    AccountId, u64, "The identifier the platform handed out."
}
fn takes(_: AccountId) {}
takes(AccountId(3));

named_enum! {
    /// Why a peer went away.
    Parting {
        /// The other end said goodbye.
        Closed = "closed",
        /// It stopped answering.
        TimedOut = "timed out",
    }
}

// The names are literals rather than the identifiers lowercased, because what
// they are for is a report a person reads.
assert_eq!(Parting::TimedOut.to_string(), "timed out");
assert_eq!(Parting::ALL, [Parting::Closed, Parting::TimedOut]);
```

`id_type!` declares the newtype, its field, its `Display` and its serde
attributes from one line, which is why it is a macro to invoke rather than a
derive to attach: a derive is handed a type that already exists. `named_enum!`
is a macro for a sharper reason. It generates `ALL` from the same list the
variants come from, and nothing in Rust makes a hand-written array grow when a
variant is added, so the two lists cannot fall out of step here the way they
would if both were written by hand.

The crate has no dependencies, and a caller supplies them instead. Every path
in an expansion that leaves the prelude is absolute -- `::core::fmt::Display`,
`::serde::Serialize` -- so the crate that expands a macro is the one that has
to have serde in scope. A `cfg` is read the same way, where the expansion lands
rather than where it was written, so `id_type!`'s encoding sits behind a
`serde` feature belonging to the caller. The `serde` feature on this crate
carries no dependency of its own and exists so that the tests here can expand
that half at all.

The derived `Hash` absorbs the number and nothing else, which is the convention
the rest of the workspace hashes under: what establishes that two peers are
reading the same field is the opening's schema, not a tag on every value.

## Scope

Two macros today. Not a proc-macro crate, and not a general newtype toolkit:
each declares one shape this workspace kept writing out by hand.
