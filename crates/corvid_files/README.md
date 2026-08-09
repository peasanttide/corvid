# `corvid_files`

The filesystem a level is read through. One trait, three failures, and a map in
memory.

```rust
use corvid_files::{Memory, Source};

let mut files = Memory::new();
files.write("level/court.bin", &[1, 2, 3])?;
assert_eq!(files.read("level/court.bin")?, [1, 2, 3]);
assert_eq!(files.list()?, ["level/court.bin"]);

// A source that takes no writes says so rather than pretending.
assert!(().write("level/court.bin", &[1]).is_err());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Why this is its own crate

`corvid_behavior` is `no_std`, and `Level::load` names a `Source` in its
signature -- so the trait has to live somewhere a `no_std` crate can reach.
`Memory` is the implementation that ships with it; anything that opens an actual
directory is a game's own, one layer up, where `std` is already paid for. This
crate has no dependencies at all.

The one allocation here is a file's bytes, and a filesystem that cannot hand
back bytes is not one.

## Sync, not async

A level is read on a loader thread, so blocking costs a tick nothing, and the
whole barrier that keeps two peers applying a level at the same tick is built on
the load being a thing that finishes rather than a thing that is polled. A
platform that has only asynchronous reads -- a browser -- is a later problem, and
this trait is where it will be solved rather than a reason to make every game's
`load` async today.

## `Missing`, `Malformed` and `ReadOnly` are different findings

A level whose file is absent is a deployment that is short a file. A level whose
file is present and will not parse is a build that disagrees with its data. Only
one of those is fixed by copying something, so they are two types rather than
two variants of one, and `Malformed: From<Missing>` crosses between them in the
one direction that makes sense.

`ReadOnly` is the third and belongs to the other direction entirely: bytes that
did not land, rather than bytes that did not arrive. Like `Missing`, it folds
every reason into one -- a source that takes no writes and a source that refused
this one say the same thing, because what a caller can act on is that the file
is not there.

## `list` is ordered, and that is load-bearing

A level built out of whatever `list` answered would otherwise have contents that
depend on a map's iteration order, and a peer that walked its props in a
different order is a peer that hashes a different level. `Memory` is a
`BTreeMap` for exactly this reason, and every other implementation owes the same
guarantee.

It takes no prefix and answers the whole source. A source is the files one level
is read through rather than a whole disk, so narrowing is the caller's `filter`
-- which keeps the one rule a source owes, sorted, from having to hold alongside
a second rule about what counts as being under a name. That second rule is the
one every implementation would have spelled differently: a string prefix and a
directory boundary disagree about `level/courtyard.bin` the moment somebody asks
for `level/court`.

A source with nothing in it answers an empty list rather than an error: "there
are no props in this level" is an answer, and a level that has to name every
file it might read is a level that cannot grow one.

## Writing is opt-in, and a shared borrow cannot do it

`Source::write` defaults to refusing with a `ReadOnly`, because most of what
implements this trait is a directory mounted for reading, an archive, or a
constant compiled into the binary. `Memory` overrides it; `()` does not, and
neither does the blanket impl on `&T` -- a `&mut &T` is a mutable borrow of the
reference rather than of what it points at, so there is no `&mut T` to forward
to and nothing to forward it with.

That is the same property from two directions. `Level::load` is handed a `&dyn
Source`, and `write` takes `&mut self`, so a level that tried to write during
its own load does not compile. The capability is not a flag a caller is asked to
consult first, either: one asked about separately can change between the
question and the write, and the answer a caller needs is the same either way --
the bytes are not there.

## Scope

The trait, its three findings, and one implementation that keeps the files in
memory. A source that opens an actual directory, reads an archive or fetches
over a network is a game's own, one layer up, where `std` is already paid for --
this crate stays `no_std` with an allocator so that a simulation crate can name
[`Source`] in a signature.

Synchronous, for the reason above, and asynchronous reads are a later problem to
solve behind this trait rather than a reason to make every game's `load` async
today. Nothing here caches, watches a path for changes, or interprets one: a
path is a key, and what it names is the source's to decide.
