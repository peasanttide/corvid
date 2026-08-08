# `corvid_macros`

The declarative macros [Corvid](https://github.com/peasanttide/corvid)'s crates
share. One so far.

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

// And it is not the other kind of identifier.
id_type! {
    /// Which account.
    AccountId, u64, "The identifier the platform handed out."
}
// `takes(seat)` would not compile.
fn takes(_: AccountId) {}
takes(AccountId(3));
```

## Why a crate rather than a module

Because a macro only one crate can reach is a pattern the next crate
reimplements slightly differently. This workspace has already had to unpick one
round of that — the same type reachable by four paths, with nothing to say which
was meant — and two spellings of "a numbered identifier" is the same shape of
problem one size down.

## Why `macro_rules!` rather than a proc macro

`id_type!` declares a type. A derive cannot: a derive is handed a type that
already exists, and what is wanted here is the newtype, its field, its
`Display` and its serde attributes from one line. A proc-macro crate would also
have brought `syn`, `quote` and a separate compilation stage into every build
below the simulation ring, to produce a newtype and a `Display`.

## No dependencies at all

A macro emits tokens; the crate that *expands* them is the one that has to be
able to name what they mention. So every path in an expansion is absolute —
`::core::fmt::Display`, `::serde::Serialize` — and a caller of `id_type!`
depends on `serde` while this crate depends on nothing.

That is what lets the whole simulation ring use it: `corvid_behavior` is
`no_std`, and so is this.

## The `Hash` is the integer and no tag

The derived `Hash` absorbs the number and nothing else, which is the convention
the rest of the workspace hashes under: what establishes that two peers are
reading the same field is the opening's schema, not a tag on every value. Two
identifiers of different kinds holding the same number therefore digest alike,
and that is fine, because nothing ever hashes one out of context.
