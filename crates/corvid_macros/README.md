# `corvid_macros`

The declarative macros Corvid's crates share. One so far: [`id_type!`], which
declares a numbered identifier.

```rust
use corvid_macros::id_type;

id_type! {
    /// Which seat, in a session's roster.
    SeatId, u16, "The position in the roster."
}

// A number, with the field public, because an identifier is a number.
let seat = SeatId(3);
assert_eq!(seat.0, 3);
assert_eq!(seat.to_string(), "3");

// And it encodes as one. `#[serde(transparent)]` is part of what the macro
// declares, so nothing on the wire records that the number was wrapped.
let json = serde_json::to_string(&seat).expect("a u16 has a json encoding");
assert_eq!(json, "3");

// It is not the other kind of identifier: `takes(seat)` would not compile.
id_type! {
    /// Which account.
    AccountId, u64, "The identifier the platform handed out."
}
fn takes(_: AccountId) {}
takes(AccountId(3));
```

`macro_rules!` rather than a derive, because a derive is handed a type that
already exists and what is wanted here is the newtype, its field, its `Display`
and its serde attributes from one line. That also keeps `syn` and `quote` out of
every build below the simulation ring.

The crate has no dependencies. A macro emits tokens, and the crate that *expands*
them is the one that has to name what they mention, so every path in an expansion
that leaves the prelude is absolute -- `::core::fmt::Display`, `::serde::Serialize`
-- and a caller of `id_type!` depends on serde while this crate depends on
nothing.

The derived `Hash` absorbs the number and nothing else, which is the convention
the rest of the workspace hashes under: what establishes that two peers are
reading the same field is the opening's schema, not a tag on every value.

## Scope

The declarative macros more than one crate needs. One so far, and the second
arrives once a shape has actually repeated -- a macro one crate needs stays in
that crate, beside the code it generates, which is where the newtype macros in
`corvid_fixed` and `corvid_vector` still are.

Not a proc-macro crate, and not a general newtype toolkit. `id_type!` declares
the one shape this workspace kept writing out by hand.
