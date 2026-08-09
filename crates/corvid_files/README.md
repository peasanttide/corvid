# `corvid_files`

The filesystem a level is read through. One trait, three failures, and a map in
memory. `no_std` with an allocator, and no dependencies.

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

[`Source`] is the trait a level is loaded through and [`Memory`] is the
implementation that ships with it. Anything that opens an actual directory,
reads an archive or fetches over a network is a game's own, one layer up, where
`std` is already paid for.

The three findings are separate types rather than variants because a caller acts
on them differently. [`Missing`] is a deployment that is short a file,
[`Malformed`] is a build that disagrees with its data, and [`ReadOnly`] is bytes
that did not land rather than bytes that did not arrive. `Malformed:
From<Missing>` crosses between the first two in the one direction that makes
sense.

[`Source::list`] answers the whole source, sorted, and that ordering is
load-bearing: a level assembled from whatever `list` returned would otherwise
depend on a map's iteration order, and a peer that walked its props in a
different order hashes a different level. It takes no prefix, because a string
prefix and a directory boundary disagree about `level/courtyard.bin` the moment
somebody asks for `level/court`; narrowing is the caller's `filter`.

Writing is opt-in. [`Source::write`] defaults to refusing, takes `&mut self`,
and is not forwarded by the blanket impl on `&T` -- so a level handed a `&dyn
Source` cannot write during its own load, and that is a compile error rather
than a flag somebody has to consult.

## Scope

The trait, its three findings, and one implementation that keeps the files in
memory. Synchronous: a level is read on a loader thread, so blocking costs a
tick nothing, and the barrier that keeps two peers applying a level at the same
tick is built on the load being a thing that finishes rather than a thing that
is polled. A platform with only asynchronous reads is a problem to solve behind
this trait rather than a reason to make every game's `load` async today.

Nothing here caches, watches a path for changes, or interprets one: a path is a
key, and what it names is the source's to decide.
