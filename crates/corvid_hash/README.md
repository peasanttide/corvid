# `corvid_hash`

The deterministic digest every [Corvid](https://github.com/peasanttide/corvid)
simulation is hashed with: sixty-four bits standing for the whole of a game
state, the same on every machine, every process, and every run.

One `u64` of state, one bijective mixer, and a `core::hash::Hasher` whose answer
does not depend on the target. No allocation, no buffer, no `std`, and no
dependencies — this crate is a leaf, and everything else in the workspace hashes
through it.

```rust
use corvid_hash::digest;

#[derive(Hash)]
struct Player { id: u16, health: i16 }

#[derive(Hash)]
struct World { tick: u64, players: Vec<Player> }

let mut world = World { tick: 0, players: vec![Player { id: 0, health: 100 }] };
let before = digest(&world);

world.players[0].health -= 1;
assert_ne!(before, digest(&world));

// A digest prints as sixteen lowercase hex digits and nothing else.
assert_eq!(digest(&1u32).to_string(), "d2ad74d3e9bb9f8b");
```

The derive is `#[derive(Hash)]`, which is `core`'s. There is no derive to
enable, no proc-macro crate in the build graph, and every type in `core` and
`alloc` that already implements `Hash` is already hashable here.

## Why not the hasher you already have

`std`'s is randomized per process. A save written on Tuesday would not load on
Wednesday, two peers would disagree on the first tick, a replay would not match
the run it recorded, and a regression test would compare two numbers that mean
nothing. Every feature this workspace is built around — save, load, replay,
rollback, desync detection, golden traces — is the same feature, which is that
the same inputs produce the same state, and a randomized hash cannot witness it.

SipHash-2-4 is the obvious fixed-key answer and can be checked against published
vectors, but it costs roughly four times as much per word, and this hash runs
over the whole simulation state every tick. So the construction here is a strong
mixer rather than a keyed MAC, its properties are tested directly rather than
matched against someone else's numbers, and its outputs are frozen as goldens so
the algorithm cannot change without a test going red.

Nothing here resists an adversary who chooses the inputs. A digest detects
divergence, not cheating; an untrusted peer is a problem for the network layer
to solve with something that is designed for it.

## What this type adds to `core::hash::Hasher`

Two things, and they are the two a shared digest needs and `Hash` cannot
provide.

**A fixed key.** `Hasher::new` seeds from a constant, where
`std::collections::hash_map::DefaultHasher` seeds from the process.

**A fixed width for every write.** The default methods on `core::hash::Hasher`
forward to `write` in *native* endian, and `write_usize` is as wide as the
target's pointer — so a `Vec`'s length prefix absorbs four bytes on `wasm32` and
eight on `x86_64`, and a browser peer desyncs from a native one on the first
tick. Every `write_*` here is overridden: integers absorb little-endian at their
declared width, and `usize` and `isize` absorb as sixty-four bits whatever the
target's pointer is.

```rust
use core::hash::Hasher as _;

let mut narrow = corvid_hash::Hasher::new();
narrow.write_u64(7);
let mut wide = corvid_hash::Hasher::new();
wide.write_usize(7);
assert_eq!(narrow.digest(), wide.digest());
```

`isize` is sign-extended to that width rather than zero-extended, which is what
keeps `-1isize` from colliding with the largest index a 32-bit target can name.

That reaches every pointer-sized integer a `Hash` implementation routes through
`write_usize` — every scalar one, and every container's length prefix. It does
not reach a `usize` stored as an *element* of a slice, which `core` hashes by a
route no `Hasher` is shown; "What the overrides do not reach", below, is about
that and about the one other thing in the same position.

## The construction

| Stage | What happens | Why |
|---|---|---|
| Seed | `state = 0x9e37_79b9_7f4a_7c15` | Any non-zero start works; the golden ratio's fractional part has no structure that interacts with the mixer |
| Absorb | `state = mix(state ^ word)` | Merkle–Damgård: one word of state, one round per word, so a gigabyte costs no memory |
| Count | `len += 8`, or the true byte count for `write` | The one thing the chain alone cannot tell you |
| Digest | `mix(state ^ len)` | Injects the count, and diffuses the last word absorbed as thoroughly as the first |

`mix` is three rounds of xor-shift and multiply by an odd constant. Both halves
are bijections modulo `2^64` — multiplication by an odd constant because it is
invertible, xor-shift because it is its own family of inverses — so the whole
function is one, and no input can be lost by cancelling against another. The
shift distances alternate between 32 and 29 so a bit folded down by one round is
folded across an unrelated boundary by the next rather than back onto itself —
which is a reason for the choice rather than a measured property of it, and
"What is tested, rather than claimed" says how far the measurement actually
reaches.

The length is what stops a chain from being trimmed. Absorbing a zero word
leaves the state changed but recoverable, so without the count `[7]` and
`[7, 0]` would be a hair apart in construction and could be made to agree;
injecting the count at the end makes them different lengths and therefore
different digests. It is also what separates a three-byte write from an
eight-byte write of the same zero-extended word, which is why `write` counts
bytes and never eight-per-round.

`digest` takes `&self` rather than consuming the hasher, because a running
simulation marks a digest every tick and does not want to rebuild the chain to
do it. `finish`, which `core::hash::Hasher` requires, is the same number as a
`u64`.

## The encoding is the wire format

A digest crosses the network, goes into save files, and is compared against
traces recorded by older builds — so what a value turns into is as much a
published format as the bytes `serde` writes. The rows below are what `core`'s
`Hash` implementations emit through this hasher, in order:

| Value | Absorbed as |
|---|---|
| `bool`, `u8`, `i8` | one byte |
| `u16` … `u128`, `i16` … `i128`, `char` | as many bytes as the type is wide, little-endian |
| `usize`, `isize` | eight bytes, whatever the target's pointer width is |
| `()`, `PhantomData<T>` | nothing at all, because a type with one value carries no information |
| Tuples, structs | each field in declaration order, no count — the arity is in the type |
| `str`, `String` | the bytes packed eight to a word, then a `0xff` byte, which is a byte no UTF-8 sequence contains |
| `[T]`, `[T; N]`, `Vec<T>` | the element count as eight bytes, then the elements — each by its own encoding, *unless* `T` is a primitive integer, for which `core` packs the whole slice as raw bytes and the next section applies |
| `BTreeMap<K, V>`, `BTreeSet<T>` | the element count, then each entry in key order |
| `Option<T>`, `Result<T, E>`, a plain enum | the variant index as eight bytes, then the payload |
| an enum carrying a `#[repr(u8)]` or any other integer `repr` | the variant index at the width that `repr` names, then the payload |
| `Digest` | one word, its raw bits, so a digest of digests is a digest |
| `&T`, `&mut T`, `Box<T>`, `Arc<T>` | whatever they point at, never the address |

Two rules generate most of that. A container whose length can vary absorbs the
length first, so a concatenation cannot be mistaken for a nesting: `(vec![1],
vec![2])` and `(vec![1, 2], vec![])` absorb different words even though their
elements do not. A type whose shape can vary absorbs a discriminant first, so
`Some(0)` and `None` differ, and so does `Some(Some(x))` from `Some(x)`.

The sequence types and the string types are two encodings, and anything
reimplementing this format has to keep them apart. A string packs its bytes and
appends a terminator; a slice counts elements and hands each element to that
element's own encoding. The same two bytes therefore digest differently
depending on which container carried them:

```rust
use core::hash::Hasher as _;
use corvid_hash::{Hasher, digest};

assert_ne!(digest("ab"), digest(&vec![b'a', b'b']));

// The string, absorbed by hand: packed bytes, then the terminator.
let mut text = Hasher::new();
text.write(b"ab");
text.write_u8(0xff);
assert_eq!(text.digest(), digest("ab"));

// The slice: an element count, then the elements.
let mut elements = Hasher::new();
elements.write_usize(2);
elements.write(b"ab");
assert_eq!(elements.digest(), digest(&vec![b'a', b'b']));
```

Nothing absorbs a type tag, so two values whose encodings coincide agree. That
is deliberate: adding a tag would cost a word per field to defend against a
confusion that cannot happen, because both peers are reading the same field of
the same type. What establishes that they are running the same types at all is
the opening's schema digest, computed once with the same function.

Declaration order is therefore the encoding, and so is an enum's `repr`.
Reordering two same-typed fields of a `#[derive(Hash)]` struct compiles, moves
every digest of that type, and is a wire break with no compile error attached to
it. Adding `#[repr(u8)]` to an enum for the sake of its layout does the same
thing by a different route: the derive hashes `discriminant_value`, whose type is
whatever the enum declares, so the `repr` narrows the variant index from eight
bytes to one and takes every digest of that type with it. Both are what the
golden tables across this workspace exist to catch.

## What the overrides do not reach

`core` implements `Hash::hash_slice` for every primitive integer by
reinterpreting the whole slice as bytes and calling `write` once, and a `Hasher`
cannot intercept it — `write` is handed bytes and is not told what they were. So
a slice of primitives does not hand its elements over one at a time. It hands
over `size_of_val` bytes, packed eight to an absorbed word, with the element
boundaries already gone. Two things follow from that, and neither is a footnote.

**Byte order.** A `Vec<u32>` absorbs the target's own order, past every
override, with nothing at the call site to see. That is not fixable here, so a
target where it would be wrong does not build: `corvid_hash` fails to compile on
a big-endian target, with a message saying why. Every target this workspace
names is little-endian, and choosing a big-endian one should be a decision taken
deliberately rather than discovered from a desync.

**Pointer width.** The same specialisation covers `usize` and `isize`, and the
bytes it hands over are the target's — four per element in a browser, eight on a
native server. A `Vec<usize>` in hashed state therefore desyncs a `wasm32` peer
from a native one, in precisely the way the overridden `write_usize` exists to
prevent, and the override cannot reach past `hash_slice` to stop it. Refusing to
build is not available as an answer to this one, because a 32-bit target is the
target this crate exists to keep in agreement. What closes it is a discipline
rather than a compiler error: hashed state names a fixed-width integer type, so
a count that crosses the wire is a `u32` or a `u64` and never a `usize`. A
container's *length* prefix is already safe, because that does go through
`write_usize`; it is a pointer-sized integer stored as an *element* that is not.

Both behaviours are pinned in `tests/width.rs`, so the ground under the refusal
and under the discipline cannot quietly move.

## What has no `Hash`, and why that is the point

`f32` and `f64` do not implement `Hash`, in `core`, and that is the guard this
crate would otherwise have had to build. The value is the target's: one unit in
the last place of difference is two digests that agree about nothing, and `0.0`
and `-0.0` compare equal while their bit patterns do not. A game whose state
holds an `f64` cannot derive `Hash` on it, so the failure is a build error on the
machine of whoever introduced it rather than a divergence found by two players.

`HashMap` and `HashSet` do not implement `Hash` either. Their iteration order
depends on a per-process random seed, so hashing one would produce a digest that
differs between two runs on the same machine. Use `BTreeMap` and `BTreeSet` in
hashed state; their order is a property of the keys rather than of the run. If a
`HashMap` is genuinely what the state wants, sort its entries and hash the
sorted sequence, and write down why.

What to reach for in place of a float is `corvid_fixed`, which is eighteen types
of fixed-point arithmetic — `I24F8` for world positions, `I16F16` for the near
field, `Factor32` for a weight, `Signed16` for a normalized axis, `Angle16` for
a heading — every one of them an integer under the skin and therefore the same
number on every target. Convert at the edge where a float is genuinely what a
value is, and hash what the conversion produced. That is a cost paid once, at a
place a person chose.

## `const` and the compile-time chain

`absorb` consumes and returns the hasher, so a chain of them is one expression
and fits where a `const` item needs one:

```rust
use corvid_hash::{Digest, Hasher};

const SCHEMA: Digest = Hasher::with_seed(0x5011_d1f1_ed5c_4e3a)
    .absorb(1)   // version
    .absorb(3)   // field count
    .digest();

assert_eq!(SCHEMA.to_string().len(), 16);
```

`with_seed` is there for exactly that: hashing the type schema under a different
seed from the state means a coincidence between the two says nothing. And
because the const interpreter and the CPU are separate implementations of the
same arithmetic, `tests/golden.rs` runs one chain both ways and asserts they
agree — which is real evidence that nothing here depends on how the host happens
to compute.

## What is tested, rather than claimed

| File | Covers |
|---|---|
| `tests/avalanche.rs` | The strict avalanche criterion cell by cell, for the mixer alone and for absorb-and-digest; 102 400 two-word inputs collide zero times; word order matters; a trailing zero word is not free; the empty digest is not zero |
| `tests/encoding.rs` | Length prefixes, discriminants, integer widths, a float that was converted to a fixed-point integer first — including the two zeroes becoming one value on the way — nesting versus flattening, the string encoding against the slice encoding, markers costing no word, insertion-order independence for the ordered collections, pointers digesting as their pointee |
| `tests/width.rs` | That `write_usize` and `write_u64` of one value agree, that `write_isize` is sign-extended to sixty-four bits, and that a `Vec`'s digest is a sixty-four-bit length prefix and its elements — the three claims a `wasm32` peer's agreement with a native one rests on — and then the one place that agreement does not hold anyway, a slice whose elements are themselves pointer-sized |
| `tests/golden.rs` | Twenty-seven frozen inputs and their exact digests — words, byte runs and strings, four of the strings not ASCII — plus structured values, a second seed, and `const` evaluation against runtime evaluation |
| `tests/derive.rs` | `#[derive(Hash)]`: field order as the encoding, a struct's fields at their declared widths, an enum's variant index before its payload and the width a `repr` narrows it to, the typed-identifier pattern, and generics with lifetimes and const parameters mixed in |
| doctests | Every Rust block in this file and in the crate's documentation |

The avalanche test measures the strict avalanche criterion per cell. For each
of the 64 × 64 pairs of an input bit and an output bit it runs 16 384 samples
and requires that output bit to flip between 45.3% and 54.7% of the time,
which is six standard deviations either side of half — wide enough that the
worst of four thousand cells does not trip it by luck, and narrow enough to be
worth asserting. The test runs it twice: once against the mixer on its own,
reachable from outside the crate because a hasher digested without absorbing
anything computes `mix(seed)` and nothing else, and once against a full absorb
and digest. A mixer cut to a single xor-shift-and-multiply round, a mixer with
its multiplies deleted, one given `3` in place of a well-chosen odd constant,
and one whose shift distances carry nothing all fail it, and they fail by the
whole sample count rather than by a hair, with cells pinned at zero or at
16 384.

Be exact about what that does not reach. Cutting the mixer from three rounds to
two — deleting a round outright, or neutering one of the three multiplies —
passes, because a two-round xor-shift-multiply mixer is genuinely a good mixer:
measured at a million samples per cell it stays inside four standard deviations
of half, which is where four thousand fair coins sit anyway. Separating two
rounds from three takes bias measurements over billions of samples, which is a
research exercise rather than something a test suite should pretend to. The
third round is margin bought for two instructions, not a property witnessed
here.

The shift distances are in the same position, and the alternation between 32 and
29 that the construction section justifies is not something this test can see.
Setting all four shifts to 32 — removing the alternation outright — measures a
worst cell 3.86 standard deviations from half at 16 384 samples and 3.75 at a
million, against 3.75 and 3.52 for the real mixer, which is the same picture the
worst of four thousand fair coins paints. What the test does catch is a distance
*below* half a word: sixteen throughout measures 16.3 standard deviations at
16 384 samples and 130 at a million, and 13 and 7 alternating pin cells outright.

It does not catch the mirror image. Sweeping the uniform-shift mixer across the
whole range, the test as written is red at 16 and below, green from 17 to 54,
red at 55, green again at 56, and red from 57 up. So 48 — as far above half a
word as 16 is below — sails through at 3.62 standard deviations, against 3.75
for the real mixer. The window this suite leaves open is 17 to 54, which is most
of the word. What is witnessed is that a distance at or below a quarter of a
word is broken and that one within a hair of the word width is broken;
everything between is indistinguishable here.

The mixer is in fact biased over more of that window than the test can see. At a
million samples per cell, 17 strays 23.7 standard deviations and 53 strays 30.8,
against 3.52 for the real mixer — so the shifts near the edges of the green band
are genuinely worse and the test is simply not looking hard enough to say so. It
is not made to look harder because a million samples per cell is thirty seconds
per assertion, and separating 29 from 31 would need far more than that. So
alternating 32 with 29 rather than with a second 32 remains a design judgement
about where a folded bit lands next, not a measured one.

What holds all of that still is `tests/golden.rs`: change a round or the odd
constant and all twenty-seven table rows go red together, and change any of
the three later shift distances and the same twenty-seven do. Changing the
*first* shift moves twenty-six of those twenty-seven, not all of them. The
survivor is the row whose input is the seed: absorbing it drives the state to
`mix(0)`, which is zero, so the digest reduces to `mix(8)` and the first shift
has nothing left to act on. `tests/golden.rs` says so where that row sits, and
keeps it, because the fixed point is worth freezing. Twenty-six rows going red
is not a gap that a twenty-eighth golden would close.

The collision test is deliberately about *multi-word* inputs. Hashing a single
word is a composition of bijections and therefore injective by construction, so
finding no collisions among a hundred thousand single words proves nothing
whatever about the mixer. Two absorbed words are the first place 128 bits of
input are squeezed into 64 bits of state and the map is honestly many-to-one,
which is why the test walks every ordered pair from a 320 × 320 grid and demands
102 400 distinct digests.

**Changing a value in `tests/golden.rs` is a wire-format break.** An algorithm
that drifts produces a desync or a refused save rather than a compile error, so
the outputs are written down as literals and a change to one is a change to the
format — a major version, and every golden in the workspace regenerated at once.

## Features

| Feature | Effect |
|---|---|
| `std` | Nothing here. The crate never reaches past `core`; the feature exists so a downstream can forward `std` across the workspace without special-casing this crate |

## Tests

```sh
cargo test -p corvid_hash --all-features
```

The avalanche and collision tests together hash a few million values, which is
worth a `--release` run rather than a debug one. Both profiles are checked in
CI, because a bit-exactness crate cannot afford a divergence between them.
