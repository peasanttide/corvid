# `corvid_wire`

The one encoding a [Corvid](https://github.com/peasanttide/corvid) snapshot is
written down in: little-endian, variable-length integers, and carrying nothing
about a value that the value did not carry itself.

Two functions, and the table helpers a golden is written with. There is no
configuration to pass, because a configuration that could be passed is a second
wire format waiting to be chosen by whoever is in a hurry.

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Shot { tick: u32, shooter: u16 }

let bytes = corvid_wire::encode(&Shot { tick: 1, shooter: 2 })?;

// The width each field was declared at is not in the bytes, which is the
// subject of "What it costs" below.
assert_eq!(bytes, [0x01, 0x02]);
assert_eq!(corvid_wire::decode::<Shot>(&bytes)?, Shot { tick: 1, shooter: 2 });

// And a capture that grew a field fails to load rather than loading as
// something it is not.
let mut grown = bytes.clone();
grown.extend_from_slice(&[0x00, 0x00]);
assert!(corvid_wire::decode::<Shot>(&grown).is_err());
# Ok::<(), corvid_wire::Error>(())
```

## What a number costs

Every integer wider than a byte is written as a variable-length quantity, and
the rule is short enough to state in full:

| The value | Written as |
|---|---|
| 0 to 250 | one byte, the number itself |
| anything larger | a marker byte, then the number little-endian at the marker's width |

The marker names the width: `fb` two bytes, `fc` four, `fd` eight, `fe` sixteen,
and the narrowest one that holds the value is the one used. A `u8` or an `i8` is
never marked -- it is its one byte, whatever it holds, which is why `ff` is
`u8::MAX` and not a marker, and why an `i8` is not zigzagged either. A signed
value *wider* than a byte is zigzagged before any of this, so `-1i32` is `01`,
`1i32` is `02`, and a small negative costs a byte rather than eight.

```rust
// A small number is one byte at every declared width.
assert_eq!(corvid_wire::encode(&1_u16)?, [0x01]);
assert_eq!(corvid_wire::encode(&1_u32)?, [0x01]);

// Past 250 the marker appears, and the payload is as narrow as it can be.
assert_eq!(corvid_wire::encode(&300_u32)?, [0xfb, 0x2c, 0x01]);
assert_eq!(corvid_wire::encode(&u32::MAX)?, [0xfc, 0xff, 0xff, 0xff, 0xff]);

// Zigzag, so the sign is in the low bit rather than in seven leading `ff`s.
assert_eq!(corvid_wire::encode(&-1_i32)?, [0x01]);

// An `i8` is the exception, being its one byte and never marked.
assert_eq!(corvid_wire::encode(&-1_i8)?, [0xff]);
# Ok::<(), corvid_wire::Error>(())
```

### What that buys

Small numbers, which is most of what a game writes down. A count in front of a
sequence is the clearest case: it is a `u64`, but it is paid once per *list*
rather than once per element, so it is three bytes on a list of ten thousand and
one byte on a list of three. Enum variant indices, seat numbers, tick numbers in
the first few hours of a session, and every counter a game keeps are all in the
one-byte range and stay there.

### What it costs

Two things, and both are worth stating plainly.

The first is a field that uses its bits. A varint is smaller than a declared
width only while the number is, and larger when it is not: a packed rotation is
a `u32` whose bits are all meaningful, so it costs five bytes rather than four,
and a `u64` digest costs nine rather than eight. A trace, which is one digest
per tick and nothing else, is therefore *larger* here than a fixed-width
encoding would make it. Fifty thousand of each, as `tests/cost.rs` measures
them:

| Fifty thousand | Bytes | Against a declared width |
|---|---|---|
| `u32` ids counting up from zero | 149,501 | 200,000 |
| `u32`s that use their high bits | 250,003 | 200,000 |
| `u64` digests | 450,003 | 400,000 |

The second, and the one that shapes how the rest of this workspace is tested: an
integer's declared width is not in the bytes. `u16(1)` and `u32(1)` are the same
single byte, so **no byte golden anywhere in this workspace can see a field
widen**. That change compiles, passes every round trip, passes every recorded
byte row -- and breaks the peer on the old build at the first field holding a
number big enough to need the extra bytes.

Two things do catch it, and neither is these bytes.

The first is the **digest**. `corvid_hash` absorbs an integer as its declared
bytes and injects the count of them at the end, so a widened field changes the
digest of every value it appears in. That is what two peers actually compare
every tick, and it is why a crate that puts a type in a snapshot records a
digest table beside its byte table. It is the strong one.

The second is a **declared schema**: a hash of a description a person wrote --
`"State.count"`, `"i64"` -- compared when a capture is loaded. It catches a
widening only if the description is edited along with the type, so it is a
description rather than a measurement. What it gives is a clean refusal at load
rather than a divergence at the first tick.

One shape escapes both, and it is worth knowing: trading width between two
fields, where one integer widens and another narrows to pay for it. The bytes are
identical and the digest is identical, because the hasher absorbs the same words
and the same total count. A declared schema is all that is left.
`tests/visible.rs` records that case as an exact value.

## What a recorded table can see

Three tables, and each is blind where the others see. This is why a crate whose
types go into a snapshot keeps more than one, and why the helpers for all three
live here.

| A recorded golden of | field order | field name | variant number | added field | **integer width** |
|---|---|---|---|---|---|
| Self-describing text (JSON) | visible | visible | invisible -- it writes names | visible | invisible |
| These bytes | visible *if the two fields' recorded values differ* | invisible | visible | visible *unless the field encodes to nothing* | invisible |
| A `corvid_hash` digest | visible *if the two fields' recorded values differ* | invisible | visible | visible | **visible** |

Every row is about a table of recorded output. None of the columns is visible to
a serialize-then-deserialize test under any format, because the writer and the
reader move together.

The qualifications on the byte row are the price of carrying no names, and JSON
does not pay them. Swapping two fields of the *same* type moves the bytes only
because their values land in the other order, so a row recorded from `{x: 1, y: 1}`
is the same row afterwards: both declarations write `0101`, while JSON writes
`{"x":1,"y":1}` and `{"y":1,"x":1}` and can tell. Adding a field is visible for
any field that writes bytes, and a `()` is not one of those -- `{x, marker: (), y}`
writes the same two bytes as `{x, y}`, where JSON gains a `"marker":null`. Both
are measured in `tests/blind.rs`.

Little-endian explicitly, on every target, matching what `corvid_hash` documents
for the digest -- a capture recorded on an aarch64 laptop is read by an x86-64
server, and "the machine's own order" is not an order.

## What it writes

| | Bytes |
|---|---|
| A `u8` or an `i8` | its one byte |
| Any wider integer | a varint, as the section above sets out |
| An `f32` or an `f64` | **its declared width**, little-endian -- four bytes or eight, whatever the value. A varint configuration does not reach floats |
| `char` | its UTF-8, one to four bytes, and no count |
| `bool` | one byte, `00` or `01` |
| A struct | its fields in declaration order, and nothing else |
| An enum | a varint variant index, then that variant's fields |
| `Option` | one tag byte, then the payload if there is one |
| A sequence, a string, a map | a varint count, then the elements, the UTF-8, or the entries as key-then-value |
| A fixed-size array | its elements, and **no count** -- the length is in the type |
| A field name, a type name | nothing |

The float row is the one that surprises people, and `tests/golden.rs` freezes it:
`1.0f32` is `0000803f`, four bytes, where `1u32` is one. Nothing about this
configuration is variable-length except integers.

A map writes its entries in whatever order it iterates, so a `HashMap` does not
write down the same bytes twice and a golden table over one is a test that fails
at random. Use a `BTreeMap`, which is what the frozen row above is.

Declaration order *is* the encoding, and so is a variant's position. Neither is
marked in a type's source as something a capture depends on, which is why a
crate that serializes anything keeps a table of what its types encode to, and
why the helper for writing that table lives here. An integer's width is *not* in
the encoding, and
that is why the same crates keep a digest table beside the byte one.

## The golden table

A byte golden is a table of labelled rows and one call. The comparison runs both
ways round -- that today's encoder writes the recorded bytes, and that the
recorded bytes still read back as the value they were recorded from -- and reports
every row that moved at once, formatted the way the table is written, so a
deliberate format change is one paste.

```rust
use corvid_wire::golden::{Row, check};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Shot { tick: u32, shooter: u16 }

/// **Changing a value in this table is a wire-format break.**
const GOLDEN: &[Row<'_>] = &[
    ("the first shot", "0102"),
    ("the second", "0202"),
];

let fixture = [
    Shot { tick: 1, shooter: 2 },
    Shot { tick: 2, shooter: 2 },
];
check("Shot", GOLDEN, &fixture).unwrap();
```

The second direction is the one a round trip cannot supply and a game actually
depends on. A serialize-then-deserialize test is symmetric: the writer and the
reader are derived from one declaration and move together, so reordering two
fields of different types, renumbering a variant and adding a field all leave it
green while changing what yesterday's capture means. Only a literal that nobody
regenerated sees any of that.

Two more comparisons live beside it, and both are here for a reason about
duplication rather than about encoding. `golden::check_digests` takes a table of
`corvid_hash` digests as `u64`, and `golden::check_text` takes a table of what a
*self-describing* format wrote. Neither is a format this crate defines and
nothing here computes a digest or writes text -- but the table above is why a
crate that puts a type in a snapshot needs all three, and a comparison written
once per crate is one that gets fixed in one place and drifts in the rest.

Freezing all three is the convention this workspace holds any type to that goes
on a wire or into a file, because a change that moves none of them has not moved
anything a peer can observe.

## Trailing bytes are refused

`decode` reads a value and then insists that the bytes are finished. This is not
strictness for its own sake: a capture that grew a field is a byte string whose
*prefix* still parses as the old type, so a decoder that stopped when it had
enough would return a value that is not what was recorded and report success. A
save file from a newer build, a snapshot from a peer one commit ahead, and a
golden row regenerated against a changed type all arrive in exactly that shape.

```rust
let bytes = corvid_wire::encode(&(1_u16, 2_u16))?;
assert_eq!(corvid_wire::decode::<(u16, u16)>(&bytes)?, (1, 2));

let mut grown = bytes.clone();
grown.push(0);
assert!(matches!(
    corvid_wire::decode::<(u16, u16)>(&grown),
    Err(corvid_wire::Error::Trailing { used: 2, len: 3 }),
));
# Ok::<(), corvid_wire::Error>(())
```

Truncation is refused by the decoder itself. Between the two, the length of a
capture is part of what it means.

## A hostile length prefix

A sequence is a count and then its elements, and a decoder reads the count
first. The count is a `u64`, so nine bytes a peer wrote can ask for sixteen
exabytes.

Holding a `&[u8]` does **not** settle that. The slice bounds what can be read; it
does not bound what can be *claimed*, and the claim is what allocates. A
container is sized from its count before a byte of its contents is read, so a
`String` whose prefix says two to the thirty-sixth is a request for sixty-four
gibibytes that is made before the slice is ever consulted. Six bytes reserve four
gibibytes; ten bytes abort the process. `tests/hostile.rs` holds the cases.

So there is a ceiling, `CEILING`, and a count past it is refused on the strength
of the number alone. It is on **both** paths, which is the part worth stating: a
limit on reading alone is the worse bug of the two, because it writes an
over-large capture without complaint and then refuses to read it back, losing a
save file at the moment somebody needs it. Here a value too large to be read back
is a value that will not be written down. `bincode` applies a configured limit to
reading only, so `encode` carries the check itself.

Two hundred and fifty-six mebibytes, against the one and a quarter that fifty
thousand entities come to. A capture that reaches it is a bug in the caller.

A transport reading from a socket needs a bound of its own, because there the
bytes have not arrived yet and a count is a request to go and get them. This
crate has no transport and does not solve that.

## Features

| Feature | Effect |
|---|---|
| `std` | Forwards `std` to `serde` and to the encoder. Adds no API. |

The crate is `no_std` with `alloc` either way, and [`Error`] implements
`core::error::Error` either way.

## Tests

```sh
cargo test -p corvid_wire --all-features
```

| File | Covers |
|---|---|
| `tests/golden.rs` | The scalars, frozen as literals: every integer width and both sides of the marker, the floats at their declared widths, `bool` and `char` |
| `tests/containers.rs` | The containers: where a length prefix goes, a count past 250 taking a marker like any other number, maps in key order, the fixed-size array that writes no count, `Option`, and nesting |
| `tests/shapes.rs` | Structs and enums, which write no name and an index -- with the two shapes this format writes alike |
| `tests/visible.rs` | Which recorded table sees each of the four changes: a reordered field, a renumbered variant and an added field in the bytes, a widened integer in the digest and not in the bytes, and a traded width in neither |
| `tests/blind.rs` | The two the table above qualifies: same-typed fields holding one value swapping unseen, and a field that writes nothing being added unseen -- with what JSON writes for both |
| `tests/trailing.rs` | That a longer and a shorter byte string are each refused and named differently, that a hostile count fails as a short one does, and that a capture larger than any ceiling worth setting still reads |
| `tests/named.rs` | Which half a type that wants its field names fails in: `#[serde(flatten)]` at `encode`, an untagged enum at `decode` |
| `tests/table.rs` | The golden helper: what it accepts, what it reports, and that its report is paste-ready |
| `tests/cost.rs` | The three sizes the "What it costs" table quotes, so the numbers in this file are measured rather than remembered |
| doctests | Every Rust block in this file |

`tests/visible.rs` is the load-bearing one. It writes a fixture down under two
declarations that differ by exactly one of the four changes and records what each
table answers -- so which change is caught by which table is a set of exact values
in the repository rather than a paragraph in a README.
