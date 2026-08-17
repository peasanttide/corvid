# `corvid_asset`

The ordered stack of data packs a level is read out of, and the digest of the
set. `no_std` with an allocator.

```rust
use corvid_asset::{Manifest, Pack, PackId, Stack};
use corvid_files::{Memory, Source};

let base = Pack::new(
    Manifest::new(PackId::new("terminus")?, "Terminus", 1),
    [
        ("materials/oak.toml", b"burns = true".to_vec()),
        ("materials/lead.toml", b"burns = false".to_vec()),
    ]
    .into_iter()
    .collect::<Memory>(),
);

// A level is a mod: it says what it needs under it, and it overrides by path.
let level = Pack::new(
    Manifest::new(PackId::new("riverside")?, "Riverside", 3)
        .requiring(PackId::new("terminus")?),
    [("materials/oak.toml", b"burns = slowly".to_vec())]
        .into_iter()
        .collect::<Memory>(),
);

let stack = Stack::mount(vec![base, level])?;
assert_eq!(stack.read("materials/oak.toml")?, *b"burns = slowly");
assert_eq!(stack.list()?, ["materials/lead.toml", "materials/oak.toml"]);
assert_eq!(
    stack.provider("materials/oak.toml").map(|pack| pack.manifest().id),
    Some(PackId::new("riverside")?),
);
# Ok::<(), Box<dyn std::error::Error>>(())
```

A [`Pack`] is a [`Manifest`] bolted to a [`Source`], and a
[`Stack`] is packs in mount order that is itself a `Source`. Nothing in a pack
is code. Modding is layering: a mod is data and assets behind a manifest,
mounted over the base, and a level is a mod like any other, which is why there
is one loader here rather than one for content and one for levels.

Index zero of a stack is the bottom. [`Source::read`] on a stack walks down from
the top and stops at the first pack holding the path, so a pack mounted later
overrides an earlier one by using the same path and by nothing else -- no
override table, no priority number, and no way for a pack to reach a file except
by naming where it lives. [`Stack::provider`] answers which pack won, because
"why did I get that material" is the question a person actually has.

[`Source::list`] on a stack is the union of every pack's listing,
de-duplicated, **sorted**. Each pack answers its own listing in order already,
but the union of several ordered lists has no order of its own, and
de-duplicating one assembled in mount order would leave the result depending on
which pack happened to define a path first. Sorting makes the listing a function
of the set of paths alone, compared as bytes, which is the same comparison on
every platform. A game that built its level out of whatever `list` returned
would otherwise hash a different level on a machine whose mods were mounted
differently.

Writing through a stack is not possible and that is structural rather than
polite. [`Pack::source`] answers a `&dyn Source`; `Source::write` takes
`&mut self` and is not forwarded by the blanket impl on `&T`; `Stack` inherits
the refusing default. So a level that tried to edit the pack it was mounted over
fails to compile.

## Mounting

[`Stack::mount`] takes the packs a session asked for and resolves an order. The
order offered is the order kept wherever [`Manifest::requires`] allows it: of
everything that could go next, the one offered earliest goes next. That is what
makes the answer a statement about what the caller asked for, since swapping two
independent packs swaps them in the stack and changes the digest. Breaking ties
by sorting identifiers instead would have made every load order alphabetical and
unable to express a preference at all. Where the requirements do disagree with
the caller, they win: a level mounted before the base it requires ends up above
it, because `requires` already says which way round the two go and making the
caller say it twice is a second thing to get wrong.

The three ways a set of packs is not a stack are [`Unmountable`] variants rather
than a walk that never finishes. A requirement nothing answers to is
[`Unmountable::Absent`], reported as itself rather than surfacing later as a
loop that is not there; one identifier claimed twice is [`Unmountable::Twice`],
because "the pack called this" has to mean one pack for every other pack's
`requires` to mean anything; and requirements that lead back to where they
started are [`Unmountable::Cycle`], which names every pack that could not be
placed rather than an arbitrary member of the loop.

## The digest

[`Stack::digest`] absorbs each pack's identifier and version, in mount order,
through `corvid_hash` and answers one value. A game puts it in its rules, where
it travels to every peer inside the opening and is compared before a tick is
simulated. A peer with an extra mod, a peer missing one, a peer on an older
version of one, and a peer who mounted the same three in a different order all
disagree with everyone else at seating and by name, instead of agreeing for
forty seconds and diverging somewhere no log explains.

The count goes in ahead of the stamps, which keeps an empty stack from colliding
with one whose single [`PackStamp`] happens to absorb to the same state. Order is
in it because a stack is a list rather than a set. It is stable across processes
and machines because `corvid_hash` is: there is no process seed and no pointer
in it, so a digest computed today is the digest computed on another peer's
laptop next year.

Nothing about the files is in that number, deliberately. It is answerable at
seating from manifests both sides already hold, and it costs no reads.
[`Pack::content`] is the other question -- a digest over every file in one pack,
in sorted path order, absorbing each path as well as its bytes -- and it catches
edited content shipped under an unchanged version. It reads the whole pack, so it
belongs to a build step or a validator rather than to a lobby.

## Scope

Manifests, mount order, path resolution, listing, and the identity digest. That
is the whole of it.

Nothing here parses a record. A manifest is `serde` behind a feature and the
format is the caller's, because TOML, JSON and a baked binary are the same
manifest to this crate; what a pack's other files mean is entirely the game's,
and the merge rules for two records with one identifier -- patch, append, delete
-- belong to whatever knows what a record is. This crate resolves paths, and a
path is a key.

Nothing here opens a directory either. [`Pack::new`] takes any `Source`, and the
one that reads a real filesystem is a game's own, one layer up, where `std` is
already paid for. Nothing caches, watches a path for changes, or reloads: a
stack is mounted once, before tick zero, and is immutable for the session,
because rollback re-simulating a tick must not be able to read different bytes
than the first pass did.

[`Source`]: corvid_files::Source
[`Source::read`]: corvid_files::Source::read
[`Source::list`]: corvid_files::Source::list
