# `corvid_hash`

The deterministic digest every Corvid simulation is hashed with: sixty-four bits
standing for the whole of a game state, the same on every machine, every process
and every run. `no_std`, no allocation, no dependencies.

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

The derive is `core`'s `#[derive(Hash)]`. There is no derive to enable and no
proc-macro crate in the build graph, so every type in `core` and `alloc` that
already implements `Hash` is already hashable here.

What [`Hasher`] adds to `core::hash::Hasher` is a fixed key and a fixed width
for every write. `std`'s hasher seeds from the process, which would mean a save
written on Tuesday failing to load on Wednesday. And the default `Hasher`
methods forward in native endian with `write_usize` as wide as the target's
pointer, so a browser peer would desync from a native one on the first tick;
every `write_*` here is overridden to absorb little-endian at its declared
width, with `usize` and `isize` at sixty-four bits whatever the target is.

The construction is a Merkle-Damgard chain over one word of state: `state =
mix(state ^ word)` per word, then `mix(state ^ len)` to finish, where `mix` is
three rounds of xor-shift and multiply by an odd constant. The length is what
separates a three-byte write from an eight-byte one of the same zero-extended
word. [`digest`](Hasher::digest) takes `&self`, because a running simulation
marks a digest every tick and does not want to rebuild the chain to do it, and
the whole chain is `const`, so a schema digest can be a `const` item.

What a value turns into is a published format, not an implementation detail: a
digest crosses the network, goes into save files and is compared against traces
from older builds. Declaration order is part of it, and so is an enum's `repr`
-- reordering two same-typed fields or adding `#[repr(u8)]` moves every digest
of that type with no compile error attached. Frozen golden tables are what catch
that.

Floats have no `Hash` in `core`, and neither do `HashMap` and `HashSet`, which
is the guard this crate would otherwise have had to build: a game whose state
holds an `f64` cannot derive `Hash` on it, so the failure is a build error
rather than a divergence found by two players. Reach for `corvid_fixed` in
hashed state and `BTreeMap` in place of `HashMap`.

## Scope

One construction, sixty-four bits wide, frozen. There is no second algorithm to
choose between, no wider digest, and no configuration beyond the seed
[`digest_with_seed`] takes: the whole job is letting two peers compare one
number per tick and agree.

Nothing here resists an adversary who picks the inputs. A digest detects
divergence, not cheating, and an untrusted peer is the network layer's problem
to solve with something designed for it. Reaching for this as a `HashMap` hasher
gives up the per-process randomization `std` has for a reason, so it is a
decision to take deliberately rather than a use this crate recommends.
