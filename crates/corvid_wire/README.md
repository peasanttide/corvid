# `corvid_wire`

The one encoding a Corvid snapshot is written down in: little-endian,
variable-length integers, and carrying nothing about a value that the value did
not carry itself. `no_std` with an allocator.

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Shot { tick: u32, shooter: u16 }

let bytes = corvid_wire::encode(&Shot { tick: 1, shooter: 2 })?;
assert_eq!(bytes, [0x01, 0x02]);
assert_eq!(corvid_wire::decode::<Shot>(&bytes)?, Shot { tick: 1, shooter: 2 });

// A capture that grew a field fails to load rather than loading as something
// it is not.
let mut grown = bytes.clone();
grown.extend_from_slice(&[0x00, 0x00]);
assert!(corvid_wire::decode::<Shot>(&grown).is_err());
# Ok::<(), corvid_wire::Error>(())
```

[`encode`] and [`decode`] take no configuration, because a configuration that
could be passed is a second wire format waiting to be chosen by whoever is in a
hurry.

An integer wider than a byte is a variable-length quantity: zero to 250 is one
byte holding the number, and anything larger is a marker naming a width -- `fb`
for two bytes, `fc` for four, `fd` for eight, `fe` for sixteen -- then the number
little-endian at that width. A `u8` or an `i8` is its one byte and never marked.
A signed value wider than a byte is zigzagged first, so `-1i32` is one byte
rather than eight. Floats are written at their declared width; nothing here is
variable-length except integers.

What that buys is the small numbers a game actually writes down: a sequence's
count, a variant index, a seat number, a tick in the first hours of a session.
What it costs is a field that uses its bits -- a packed rotation is five bytes
rather than four, a digest nine rather than eight -- and, more importantly, that
an integer's declared width is not in the bytes. `u16(1)` and `u32(1)` are the
same single byte, so no byte golden anywhere can see a field widen. What catches
that is the digest, which absorbs an integer at its declared width, and it is why
a crate that puts a type in a snapshot records a digest table beside its byte
table.

[`decode`] insists the bytes are finished when the value is, because a capture
that grew a field is a byte string whose prefix still parses as the old type. And
a sequence's count is a `u64`, so nine bytes a peer wrote can ask for sixteen
exabytes: [`CEILING`] bounds it on both paths, since a limit on reading alone
writes an over-large capture and then refuses to read it back.

[`golden`] is the table helper a frozen encoding is written with. It compares both
ways round -- that today's encoder writes the recorded bytes, and that the
recorded bytes still read back as the value they came from -- because a
serialize-then-deserialize test moves the writer and the reader together and sees
neither.

## Scope

Two functions, and the table helpers a golden is written with. No builder, no
second format, no compression and no encryption: those are layers over a byte
string, and this crate is what produces the byte string.

Not self-describing. A stream carries no field names, no type tags and no schema,
so a reader has to hold the same declarations the writer did, and a version in
front of a capture is the game's to write.
