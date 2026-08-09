# `corvid_macros`

The declarative macros [Corvid](https://github.com/peasanttide/corvid)'s crates
share.

```rust
use corvid_macros::id_type;

id_type! {
    /// Which seat, in a session's roster.
    SeatId, u16, "The position in the roster."
}

// A number, with the field public, because an identifier is a number.
let seat = SeatId(3);
assert_eq!(seat.0, 3);

// The display names the type, because which kind of identifier a number is
// is the thing the newtype exists to keep straight.
assert_eq!(seat.to_string(), "SeatId(3)");

// And it encodes as one. `#[serde(transparent)]` is part of what `id_type!`
// declares, so nothing on the wire records that the number was wrapped.
let json = serde_json::to_string(&seat).expect("a u16 has a json encoding");
assert_eq!(json, "3");
assert_eq!(serde_json::from_str::<SeatId>(&json).expect("it reads back"), seat);

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
round of that -- the same type reachable by four paths, with nothing to say which
was meant -- and two spellings of "a numbered identifier" is the same shape of
problem one size down.

## Why `macro_rules!` rather than a proc macro

`id_type!` declares a type. A derive cannot: a derive is handed a type that
already exists, and what is wanted here is the newtype, its field, its
`Display` and its serde attributes from one line. A proc-macro crate would also
have brought `syn`, `quote` and a separate compilation stage into every build
below the simulation ring, to produce a newtype and a `Display`.

## No dependencies at all

A macro emits tokens; the crate that *expands* them is the one that has to be
able to name what they mention. So every path in an expansion that leaves the
prelude is absolute -- `::core::fmt::Display`, `::serde::Serialize` -- and a
caller of `id_type!` depends on `serde` while this crate depends on nothing.
The nine built-in derives are left bare, as they are in the newtype macros in
`corvid_fixed` and `corvid_vector`, because a prelude name needs no help: they
resolve even inside a module marked `#![no_implicit_prelude]`, which the tests
pin.

`serde` and `serde_json` do appear under `[dev-dependencies]`, and that is the
same rule read from the other side rather than an exception to it: the doctest
above and every test in `tests/` is itself a crate that expands `id_type!`, so
each has to supply the serde the expansion names. Nothing downstream of this
crate inherits them.

That is what lets the whole simulation ring use it: `corvid_behavior` is
`no_std`, and so is this.

## The `Hash` is the integer and no tag

The derived `Hash` absorbs the number and nothing else, which is the convention
the rest of the workspace hashes under: what establishes that two peers are
reading the same field is the opening's schema, not a tag on every value. An
identifier therefore digests exactly as the bare integer inside it does, and two
identifiers of the same width holding the same number digest alike; that is
fine, because nothing ever hashes one out of context.

Two of *different* widths feed the hasher different bytes, and the pair in the
example above is exactly that case: `Hash for u16` writes two where `Hash for
u64` writes eight. That is a claim about the input and not about the digest --
a `Hasher` is free to collide on any two inputs and none of them promises
otherwise, so "these two cannot come out alike" is not something this crate or
`Hash` will tell you. Read the difference in what is written as an accident of
the reprs rather than as the type tag returning: widen the `u16` and even the
input is the same again. Nothing should be built on it either way.
