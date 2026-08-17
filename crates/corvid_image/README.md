# `corvid_image`

Pictures as data, and the plan for getting a very large one onto a GPU.

```rust
use corvid_image::{
    PixelFormat, SourceView, TileConfig, TilePlanner, VramBudget, extent,
};

let mut planner = TilePlanner::new(TileConfig::MIN_SPEC)?;
let plate = planner.register(extent(16384, 16384), PixelFormat::SRGB8)?;

let plan = planner.plan(&[SourceView::full(plate)], VramBudget::MIN_SPEC.share(10));
planner.commit(&plan);

// The middle of the plate now resolves to a tile, at whatever zoom the budget
// left resident there.
let hit = plan.table().resolve(plate, [0.5, 0.5]);
assert!(hit.is_some_and(|hit| hit.texel[0] < TileConfig::MIN_SPEC.tile_size));
# Ok::<(), Box<dyn core::error::Error>>(())
```

`no_std` plus `alloc`. It is the client ring: nothing here is hashed, nothing
here crosses the wire, and two machines are allowed to disagree about every
number in it, which is what lets a viewport be an `f32`. It names no graphics
library at all. Putting a tile on a device is `corvid_image_render`'s job, and
it is a separate crate for exactly that reason.

## An image is a size, a format and a buffer

[`Image`] holds one level: an [`Extent`], a [`PixelFormat`] of one to four
eight-bit channels in either sRGB or linear, and tightly packed rows with no
stride, because a stride is a device's business. [`Extent::mip_levels`] and
[`Extent::mip`] describe the pyramid over it, and nothing here builds one: a box
filter over a 131072-texel plate is work a GPU finishes while a CPU is still
deciding which cache line to fault, so the mips are the device half's to
generate and this side only says how many there are and how big each is.

[`decode`] turns a file into an [`Image`], and [`Codec::sniff`] says what a file
is. Both exist in every build; the `png` and `jpeg` features decide which ones
can actually be carried out, and a build missing the codec for a format it
recognises answers [`DecodeError::NoDecoder`] naming the format rather than
pretending the bytes are gibberish.

The third format the archive holds is JPEG 2000, which is what a georeferenced
map plate is published as, and there is no decoder behind a feature for it
because there is no pure-Rust JPEG 2000 decoder to put there. What exists is
bindings to `OpenJPEG`, which is C, which would make this crate an FFI crate and
by this workspace's own rules a different crate entirely. So [`Codec::Jpeg2000`]
is recognised and never decoded, and
[`Codec::is_decodable`](Codec::is_decodable) is permanently false for it, so a
pipeline can say "this plate needs converting" rather than "these bytes are not
an image". Convert the plates to PNG when they are baked.

## The tile plan is what this crate is really for

Some plates are over sixteen thousand texels a side and the ceiling here is
131072, which is larger than any texture any device will allocate. So an image
is cut into [`TILE_SIZE`]-texel tiles, some tiles are resident, and a lookup
table tells the fragment shader which. [`TileConfig`] holds the four numbers
that shape all of it -- the tile side, the resident tile budget, the source
count and the largest addressable image -- and [`TileConfig::MIN_SPEC`] is the
design's minimum specification rather than a constant baked into the arithmetic.

A [`TilePlanner`] knows what exists and what is resident. [`TilePlanner::register`]
takes a picture's size and format, never its bytes, and refuses one past
[`TileConfig::max_image_size`] with [`TileError::TooLarge`] instead of clipping
it, because a map quietly missing its eastern third still looks exactly like a
map. [`TilePlanner::plan`] then takes what the viewer can see and how much
device memory the cache may have, and answers a [`TilePlan`]: the
[`uploads`](TilePlan::uploads), the [`evictions`](TilePlan::evictions), and the
[`table`](TilePlan::table) the result will be sampled through. Nothing in it has
touched a device. The device half performs the evictions, then the uploads, then
calls [`TilePlanner::commit`].

## The table, and the arithmetic a shader does to it

[`TileTable`] is one layer per source, each `side` pages square, where a page is
one tile's worth of level-zero texels and `side` is the widest registered source
measured in them. Each entry is one 32-bit word: twelve bits of slot, four of
zoom, and eight each for the two halves of the page's offset inside its tile.
The shader scales the uv by the source's extent, shifts to get a page, loads one
word, and shifts again:

```text
texel = uv * extent
page  = texel >> log2(tile_size)
entry = words[layer * side * side + page.y * side + page.x]
inner = (entry.offset * tile_size + (texel & (tile_size - 1))) >> entry.level
```

[`TileTable::resolve`] is that, in Rust and to the texel, so the addressing can
be checked against a hand-worked answer on a machine with no GPU in it. The
final shift by the entry's zoom is what makes a coarse fallback work: a tile
four levels up covers sixteen times the ground, so a texel inside it is sixteen
times closer to its origin.

The eight bits of offset are what cap the pyramid at
[`TileConfig::MAX_LEVEL`], since a tile at level `L` spans `2^L` pages. That is
the price of the entry being one word, and it means a source wider than
`tile_size << 8` bottoms out at a small grid of tiles rather than a single one.

## Degrading instead of stalling

A [`Priority`] is a [`Tier`], a quantised weight and a zoom, compared in that
order and oriented so that larger means keep. Root tiles -- the top of each
visible source's pyramid -- outrank everything, so a second source cannot be
starved to nothing by a first one with a large working set. Below that the
viewer's own weighting decides between sources, and within one source a coarser
tile always outranks a finer one. That last ordering is the whole trick: when
the budget cannot hold the working set, what gets evicted is detail, and
[`Residency::nearest_at_or_coarser`] serves the page from the blurrier tile that
is still there. A frame that is soft now beats a frame that is sharp two frames
from now, and [`TilePlan::is_degraded`] says which one happened.

The weights are quantised to sixteen bits before anything compares them because
`f32` has no total order, and a plan sorted by raw weights would be at the mercy
of whichever `NaN` reached a comparator first. Everything a plan walks is a
[`BTreeMap`](alloc::collections::BTreeMap) or a slice sorted by a total order
ending in [`TileKey`], so two calls with the same input answer the same plan
down to the slot numbers. A streamer that reshuffles under you is a bug, not a
heuristic.

## What this crate will not cover

No device, no queue, no texture, no shader: `corvid_image_render` is the other
half and this one must be usable, and testable, on a machine with no graphics
stack. No resampling and no mip generation, for the same reason -- the plan says
which mip of which tile, and the device makes it. No camera and no projection:
[`SourceView`] is three numbers the caller computes from whatever it is looking
through, so a lens, a minimap and a full-screen map are the same input to this
crate and none of them makes it depend on the platform ring.

No encoding. Nothing here writes a PNG, because writing one is a capture seam
and `corvid_render` already owns that. No compressed texture formats: block
compression is a bake-time decision about what goes in a pack, and this crate
plans residency for whatever is already there.

No fetching. A [`TilePlan`] names tiles; where their bytes come from -- a pack,
a file, a network -- is the caller's, and the whole point of answering a value
rather than performing an action is that the caller gets to decide how
asynchronous that is.
