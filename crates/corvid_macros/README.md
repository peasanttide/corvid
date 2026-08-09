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

`id_type!` declares the newtype, its field, its `Display` and its serde
attributes from one line, which is why it is a macro to invoke rather than a
derive to attach: a derive is handed a type that already exists.

The crate has no dependencies, and a caller of `id_type!` supplies them instead.
Every path in the expansion that leaves the prelude is absolute --
`::core::fmt::Display`, `::serde::Serialize` -- so the crate that expands the
macro is the one that has to have serde in scope.

The derived `Hash` absorbs the number and nothing else, which is the convention
the rest of the workspace hashes under: what establishes that two peers are
reading the same field is the opening's schema, not a tag on every value.

## Scope

One macro today. Not a proc-macro crate, and not a general newtype toolkit:
`id_type!` declares the one shape this workspace kept writing out by hand.
