# `corvid_asset`

Loading, caching, placeholders and levels of detail — on the platform ring,
where the files are.

A `Handle<T>` is `Clone + Debug` and nothing else. It is not `Hash`, not `Eq`
and not `Serialize`, so it cannot satisfy `corvid_behavior::Data` and cannot be
put in a `State`. The compiler refuses the mistake; there is no review note to
miss. `Handle`'s own documentation carries the `compile_fail` that says so.

The tick names a `Ref`, the runtime resolves it, and the handle stays on this
side of the boundary:

```rust
use corvid_asset::{Asset, Assets, Lod, Malformed, Memory};

/// The level, as its author wrote it.
#[derive(Debug, Default, PartialEq)]
struct Rooms(Vec<String>);

impl Asset for Rooms {
    fn placeholder() -> Self {
        Self::default()
    }

    fn decode(bytes: &[u8], _lod: Lod) -> Result<Self, Malformed> {
        let text = str::from_utf8(bytes).map_err(|_| Malformed::new("not utf-8"))?;
        Ok(Self(text.lines().map(str::to_owned).collect()))
    }
}

let mut memory = Memory::new();
memory.insert("levels/terminus", b"hall\ncellar".to_vec());

let assets = Assets::new(Box::new(memory));

// Asking answers immediately, with the placeholder.
let level = assets.load::<Rooms>("levels/terminus");
assert!(!level.is_resident());
assert_eq!(level.lod(), Lod::PLACEHOLDER);

// The loader reads and decodes off the frame thread; `poll` installs.
while !assets.is_settled() {
    assets.poll();
}

assert!(level.is_resident());
assert_eq!(level.lod(), Lod::FINEST);
assert_eq!(level.get().0, ["hall", "cellar"]);

// Asking again shares the one asset rather than reading twice.
assert_eq!(assets.load::<Rooms>("levels/terminus").holders(), 3);
```

`get` always answers. A renderer that had to branch on whether its mesh arrived
would branch every frame for the whole life of the program to cover a case that
lasts two hundred milliseconds; `is_resident` exists for the loading screen,
which is the one caller that cares.

## Where bytes come from

`Source` has two implementations. `Files` is a directory on disk, and it
resolves every path *under* its root — a `..` component, a leading separator and
a drive prefix are refused rather than followed. `Memory` is a map from path to
bytes, and it is public API rather than a test helper: a golden test that loads
a level with no file present is exactly the case a determinism workspace has.

## Levels of detail

`Asset::levels` says how many a kind has. The loader reads the path once and
decodes it at every level, coarsest first, and `poll` installs one level per
asset per call — so a chain of four promotes over four frames and `lod()`
reports each in turn. `Lod::FINEST` is zero and `Lod::PLACEHOLDER` is
`u8::MAX`, so the derived order runs from detailed to crude and a promotion is a
decrease.

## Loading is a thread and a channel

Not an async runtime. `Assets` owns one worker that reads through the `Source`
and posts decoded levels back; `poll` drains the queue on the frame thread, so
nothing is installed halfway through a frame. A game already running an executor
runs one more thread, which is the cheaper of the two mistakes. Where the
operating system refuses a thread, `poll` runs the queue itself.

## Eviction

The cache holds one strong reference per asset, so dropping every handle does
not free it — `evict` does, and it takes only what nothing else is holding. A
`Weak` names an asset without being a reason to keep it, and `upgrade` answers
`None` once the eviction has happened.
