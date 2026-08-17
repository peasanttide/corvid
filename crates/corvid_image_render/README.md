# `corvid_image_render`

The device half of a `corvid_image` tile plan: an array texture full of tiles,
a lookup table a fragment shader loads one word out of, and the WGSL that turns
a source id and a uv into a texel.

```rust
use corvid_image::{PixelFormat, TileConfig, VramBudget};
use corvid_image_render::{TileCache, WGSL};

# fn build(device: &wgpu::Device) -> Result<(), Box<dyn core::error::Error>> {
let cache = TileCache::new(
    device,
    TileConfig::MIN_SPEC,
    VramBudget::MIN_SPEC.share(25),
    PixelFormat::SRGB8,
    "map",
)?;

// A game's own shader, with this crate's addressing concatenated onto it.
let mine = "@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return corvid_tile_sample(0u, uv);
}";
let source = format!("{WGSL}\n{mine}");
assert!(source.contains("corvid_tile_lookup"));
assert!(cache.slots() > 0);
# Ok(())
# }
```

`corvid_image` is the half with no device in it: it registers sources, ranks
what the viewer can see, and answers a [`TilePlan`] of uploads, evictions and a
[`TileTable`]. This crate is the half that has one. It is the client ring, so
nothing here is hashed and nothing here crosses the wire, and it names real
`wgpu` because `corvid_render` hands out a real `wgpu::Device` and this
workspace does not abstract over GPUs.

## Where `wgpu` comes from

This crate's manifest names `wgpu` directly, as `wgpu = { workspace = true }`,
rather than reaching it through a re-export from `corvid_render` -- `corvid` is
the workspace's one facade, and no crate below it forwards its neighbours.

**The pin is what keeps the version single, not the re-export.** One entry in
the root manifest, one version in the graph, one `raw-window-handle` that a
surface and a window can agree on. So the rule is not "do not name it" but "do
not name a *version* of it", and `tests/manifest.rs` is the mechanical form of
that one: it reads this crate's own manifest and fails on a `wgpu` line that
carries a version rather than `workspace = true`.

## Why this crate owns a shader snippet

[`WGSL`] is source text a caller concatenates onto their own shader, and
[`wgsl_at`] is the same text with its bind group moved. It ships here rather
than in a game because the layout of a [`TileEntry`] -- twelve bits of slot,
four of zoom, eight each for the two halves of a page offset -- and the code
that unpacks it are one fact written twice, and two crates holding one fact is
how the two stop agreeing. The same goes for the atlas: which layer a slot is
in and where its tile starts there is a decision [`Atlas`] made from a device's
limits, and the shader is told it through a uniform rather than assuming it.

[`TileTable::resolve`] is the same arithmetic in Rust and to the texel.
`tests/device.rs` runs both against one another on a real adapter, which is the
only way a shader's addressing is ever actually checked.

## The four steps a plan is carried out in

[`TileCache::evict`] frees the slots the plan gave up. [`TileCache::upload`]
writes the tiles it asked for, one texture write each, at level zero only.
[`TileCache::generate_mips`] records the rest of the chain into the caller's
encoder. [`TileCache::write_table`] publishes the table describing the result.
Only then does the caller call [`TilePlanner::commit`].

That order is load-bearing in both directions. The planner hands a freed slot
straight back out as somewhere to upload to, so uploading before evicting
overwrites a tile the table still points at -- and [`TileCache::upload`] answers
[`CacheError::SlotOccupied`] rather than doing it. Publishing the table before
the tiles have landed is the same mistake pointing the other way: a frame that
samples slots whose contents are still in flight.

Nothing here is asynchronous and nothing here fetches. A [`TilePlan`] names
tiles; where their bytes come from -- a pack, a file, a network -- is the
caller's, and how many of them arrive this frame is the caller's too. Upload
what you have, and the pages you did not get stay served from the coarser tile
that is still resident, which is what [`TilePlan::is_degraded`] reports.

## Why a layer is a grid of tiles

A device opened with this workspace's limits allows 256 array layers, and the
design's minimum specification asks for 2048 resident tiles. So a layer holds a
square-ish grid of tiles and there are a few layers, and a slot becomes a layer
and an origin by two shifts and two masks -- which is what both grid dimensions
being powers of two is for. [`Atlas::plan`] makes that choice from
[`wgpu::Limits`] alone, so it can be checked on a machine with no adapter.

The mip chain follows from the same alignment. Every level of a layer is a whole
number of tiles and every tile is a power of two, so the 2x2 block under a
destination texel is always inside one tile: the reduction needs no clamp and
cannot average two tiles together. It runs as one render pass per layer per
level with a scissor per tile, which is eight passes and a hundred draws for a
frame that uploaded a hundred tiles rather than eight hundred passes.

What the packing costs is a seam. Sampling at a tile's edge would filter into
whatever is packed beside it, so `corvid_tile_point` clamps the sample into the
tile's own interior, and two adjacent tiles therefore do not interpolate across
their shared edge. A gutter would fix it and would cost a border texel of every
tile on every upload; this crate has no gutter and says so rather than pretending
the seam is not there.

## The budget, and how much of it is a guess

[`vram_budget`] answers what the cache may occupy on an adapter: [`CACHE_SHARE`]
of what the card appears to have, never below [`FLOOR`], which is that share of
the design's minimum specification. A machine below the minimum specification is
not a machine the budget shrinks for -- it is a machine that does not meet the
minimum specification, and a cache that quietly halved itself there would turn a
blurry map into a bug nobody can find.

**How much the card has is an estimate, and there is no exact answer to be had.**
`wgpu` exposes no memory size at all, because WebGPU deliberately does not: a
page that could read how much memory a card has could fingerprint it. So
[`vram_estimate`] reads `wgpu::Limits::max_buffer_size`, which every desktop
driver bounds by its device-local heap, and corrects it by device type -- a
discrete GPU at its word, an integrated one halved because that memory is shared
with the system, and a software rasteriser given the minimum specification flat
because system memory is not a texture budget. The correction is coarse on
purpose: being wrong costs a cache one step blurrier or one step larger than it
might have been, and more decimal places would not make it truer.

[`resident_tiles`] is how a budget becomes a tile count, and it is not
[`VramBudget::capacity`]. Two things a device costs that a file does not are in
it: a mip chain is a third again, and a three-channel scan is four bytes a texel
once it is a texture, because no device has a three-byte texel.

## What this crate will not cover

No planner and no policy. What is worth keeping, what to give up and at what
zoom to serve a page are `corvid_image`'s, and every one of them is decided
without a device on purpose. This crate performs a plan; it never makes one, and
it holds no opinion about a plan it disagrees with beyond refusing an upload to
a slot whose eviction never happened.

No fetching, no decoding and no asynchrony. [`TileCache::upload`] takes an
[`Image`] that already exists; getting one is the caller's, and so is deciding
how many to get this frame.

No compressed texture formats. Block compression is a bake-time decision about
what goes in a pack, and a cache that transcoded on upload would be doing on
every machine what should have been done once. No gutters and no anisotropic
filtering, for the reason above: both are costs paid per tile for a seam that a
map drawn at or near its native resolution does not show.

No window, no surface, no pass and no draw call for the caller's own geometry.
This crate hands out a bind group layout, a bind group and a shader snippet;
what is drawn with them is the game's.

[`Image`]: corvid_image::Image
[`TileEntry`]: corvid_image::TileEntry
[`TilePlan`]: corvid_image::TilePlan
[`TilePlan::is_degraded`]: corvid_image::TilePlan::is_degraded
[`TilePlanner::commit`]: corvid_image::TilePlanner::commit
[`TileTable`]: corvid_image::TileTable
[`TileTable::resolve`]: corvid_image::TileTable::resolve
[`VramBudget::capacity`]: corvid_image::VramBudget::capacity
