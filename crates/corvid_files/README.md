# `corvid_files`

The filesystem a level is read through. One trait, two failures, and a map in
memory.

```rust
use corvid_files::{Memory, Source};

let mut files = Memory::new();
files.insert("level/court.bin", vec![1, 2, 3]);
assert_eq!(files.read("level/court.bin")?, [1, 2, 3]);
# Ok::<(), corvid_files::Missing>(())
```

## Why this is its own crate

`corvid_behavior` is `no_std`, and `Level::load` names a `Source` in its
signature — so the trait has to live somewhere a `no_std` crate can reach.
Everything that opens an actual directory is `corvid_asset`'s `Files`, one layer
up, where `std` is already paid for. This crate has no dependencies at all.

The one allocation here is a file's bytes, and a filesystem that cannot hand
back bytes is not one.

## Sync, not async

A level is read on a loader thread, so blocking costs a tick nothing, and the
whole barrier that keeps two peers applying a level at the same tick is built on
the load being a thing that finishes rather than a thing that is polled. A
platform that has only asynchronous reads — a browser — is a later problem, and
this trait is where it will be solved rather than a reason to make every game's
`load` async today.

## `Missing` and `Malformed` are different findings

A level whose file is absent is a deployment that is short a file. A level whose
file is present and will not parse is a build that disagrees with its data. Only
one of those is fixed by copying something, so they are two types rather than
two variants of one, and `Malformed: From<Missing>` crosses between them in the
one direction that makes sense.

## `list` is ordered, and that is load-bearing

A level built out of whatever `list` answered would otherwise have contents that
depend on a map's iteration order, and a peer that walked its props in a
different order is a peer that hashes a different level. `Memory` is a
`BTreeMap` for exactly this reason, and every other implementation owes the same
guarantee.

A prefix with nothing under it answers an empty list rather than an error:
"there are no props in this level" is an answer, and a level that has to name
every file it might read is a level that cannot grow one.
