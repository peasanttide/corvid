// Resolving a source id and a uv through the tile table, in the fragment
// shader that draws with the result.
//
// This is `corvid_image_render::WGSL`. A caller concatenates it into their own
// shader source rather than compiling it here, because a shader has one set of
// entry points and they are the caller's. What it declares is one bind group --
// the one `TileCache::layout` describes and `TileCache::bind_group` fills --
// and four functions over it.
//
// It lives in this crate because every constant below is either a field of a
// `corvid_image::TileEntry` or a number `TileCache` chose when it laid the
// atlas out, and a second copy of them in a game's shader is a copy that stops
// agreeing the first time either side moves. `TileTable::resolve` is the same
// arithmetic in Rust; `tests/device.rs` runs both and compares them to the
// texel.

// How many sources the parameter block has room for.
//
// A uniform array's length is a compile-time constant in WGSL, so this is fixed
// rather than configured. `corvid_image::MAX_NUM_MAPS` is 255 and
// `TileConfig::validate` refuses more, which is what makes 256 enough for every
// configuration there is.
const CORVID_TILE_SOURCES: u32 = 256u;

// The three fields of a `corvid_image::TileEntry`, low to high: twelve bits of
// slot, four of zoom, then eight each for the two halves of the page's offset
// inside its tile.
const CORVID_TILE_SLOT_MASK: u32 = 4095u;
const CORVID_TILE_LEVEL_SHIFT: u32 = 12u;
const CORVID_TILE_LEVEL_MASK: u32 = 15u;
const CORVID_TILE_OFFSET_U_SHIFT: u32 = 16u;
const CORVID_TILE_OFFSET_V_SHIFT: u32 = 24u;
const CORVID_TILE_OFFSET_MASK: u32 = 255u;

// The all-ones slot is "nothing resident covers this page, at any zoom", which
// is what every entry of a freshly built table holds.
const CORVID_TILE_ABSENT: u32 = 4095u;

struct CorvidTileParams {
    // x: the tile side in texels. y: the log of it, which is what turns a texel
    // into a page. z: the table's side in pages. w: how many sources are
    // registered.
    shape: vec4<u32>,
    // x: the log of how many tiles one atlas layer is across. y: the log of how
    // many tiles one layer holds. z and w: one layer's size in texels.
    atlas: vec4<u32>,
    // Per source, xy is its size in texels at level zero and zw is unused: a
    // uniform array's stride is sixteen bytes whatever its element is.
    sources: array<vec4<u32>, CORVID_TILE_SOURCES>,
};

@group(1) @binding(0) var<uniform> corvid_tile_params: CorvidTileParams;
// One layer per source, each `shape.z` pages square, one `i32` a page. A
// texture rather than a storage buffer so that the one indexed load a sample
// costs is a texture fetch, which every backend in this workspace's reach has
// and which needs no alignment rules kept in step on the Rust side.
@group(1) @binding(1) var corvid_tile_table: texture_2d_array<i32>;
// The tiles themselves: a grid of them per layer, mip chain and all.
@group(1) @binding(2) var corvid_tile_atlas: texture_2d_array<f32>;
@group(1) @binding(3) var corvid_tile_sampler: sampler;

// Where one sample of one source lands.
//
// `texel` is `corvid_image::TileSample::texel` exactly -- the texel inside the
// tile, in `0..tile_size` -- so a caller can compare a frame against the CPU's
// answer. `origin` and `layer` are where that tile sits in the atlas, which is
// what turns the texel into something to sample.
struct CorvidTileHit {
    // False when nothing resident covers this page at any zoom. Everything else
    // in the struct is zero when it is false.
    present: bool,
    // The zoom actually serving this page, which is not necessarily the one the
    // view asked for: a page whose detail has not arrived is served from a
    // coarser tile that is already there.
    level: u32,
    // Which array layer of the atlas.
    layer: u32,
    // Where the tile starts in that layer, in texels.
    origin: vec2<u32>,
    // The texel inside the tile, in `0..tile_size`.
    texel: vec2<u32>,
};

// How big a source is in texels, or zero for one the table does not hold.
fn corvid_tile_extent(source: u32) -> vec2<u32> {
    if source >= corvid_tile_params.shape.w || source >= CORVID_TILE_SOURCES {
        return vec2<u32>(0u, 0u);
    }
    return corvid_tile_params.sources[source].xy;
}

// The fragment shader this crate exists for: a uv on a source becomes a tile
// and a texel inside it.
//
//   texel = uv * extent
//   page  = texel >> log2(tile_size)
//   entry = table[source][page]
//   inner = (entry.offset * tile_size + (texel & (tile_size - 1))) >> entry.level
//
// The final shift by the entry's zoom is the whole of what makes a coarse
// fallback work: a tile four levels up covers sixteen times the ground, so a
// texel inside it is sixteen times closer to its origin.
//
// A uv outside [0, 1) is clamped to the edge texel, which is what a clamped
// sampler does and what stops a rounding error at the seam between two plates
// from reading the wrong page.
fn corvid_tile_lookup(source: u32, uv: vec2<f32>) -> CorvidTileHit {
    var hit: CorvidTileHit;
    hit.present = false;
    hit.level = 0u;
    hit.layer = 0u;
    hit.origin = vec2<u32>(0u, 0u);
    hit.texel = vec2<u32>(0u, 0u);

    let extent = corvid_tile_extent(source);
    if extent.x == 0u || extent.y == 0u {
        return hit;
    }

    let tile = corvid_tile_params.shape.x;
    let page_shift = corvid_tile_params.shape.y;
    let side = corvid_tile_params.shape.z;

    let scaled = floor(uv * vec2<f32>(extent));
    let limit = vec2<f32>(extent - vec2<u32>(1u, 1u));
    let texel = vec2<u32>(clamp(scaled, vec2<f32>(0.0, 0.0), limit));
    let page = texel >> vec2<u32>(page_shift, page_shift);
    if page.x >= side || page.y >= side {
        return hit;
    }

    // One indexed load, and everything the sample needs is in it. `bitcast`
    // rather than a conversion because the word is a bit pattern: the table is
    // `i32` because that is what a signed texture holds, and nothing does
    // arithmetic on the sign.
    let word = bitcast<u32>(
        textureLoad(corvid_tile_table, vec2<i32>(page), i32(source), 0).x
    );
    let slot = word & CORVID_TILE_SLOT_MASK;
    if slot == CORVID_TILE_ABSENT {
        return hit;
    }
    let level = (word >> CORVID_TILE_LEVEL_SHIFT) & CORVID_TILE_LEVEL_MASK;
    let offset = vec2<u32>(
        (word >> CORVID_TILE_OFFSET_U_SHIFT) & CORVID_TILE_OFFSET_MASK,
        (word >> CORVID_TILE_OFFSET_V_SHIFT) & CORVID_TILE_OFFSET_MASK,
    );
    let inside = texel & vec2<u32>(tile - 1u, tile - 1u);
    let inner = (offset * tile + inside) >> vec2<u32>(level, level);

    // Slot to layer and origin: two shifts and two masks, which is what both
    // grid dimensions being powers of two buys.
    let across_shift = corvid_tile_params.atlas.x;
    let layer_shift = corvid_tile_params.atlas.y;
    let cell = slot & ((1u << layer_shift) - 1u);
    let column = cell & ((1u << across_shift) - 1u);
    let row = cell >> across_shift;

    hit.present = true;
    hit.level = level;
    hit.layer = slot >> layer_shift;
    hit.origin = vec2<u32>(column, row) * tile;
    hit.texel = inner;
    return hit;
}

// Where to sample a hit, as the normalized coordinate a sampler takes.
//
// `lod` is the mip level the caller is about to sample at, and it is an
// argument rather than something read from the hit because a mip is a screen
// decision: the table already chose which *tile*, and this chooses how blurry
// to read it.
//
// The point is clamped to the tile's own interior, which is what stops a filter
// from reaching into the tile packed beside it in the atlas. That clamp is the
// price of packing a grid into one layer instead of spending a layer per tile,
// and what it costs is a seam: two adjacent tiles do not interpolate across
// their shared edge. A gutter would fix that and would cost a texel of every
// tile's border on every upload; this crate has no gutter and says so.
fn corvid_tile_point(hit: CorvidTileHit, lod: f32) -> vec2<f32> {
    let tile = corvid_tile_params.shape.x;
    let size = vec2<f32>(corvid_tile_params.atlas.zw);
    // A texel at mip `lod` is `2^lod` texels wide down here and a bilinear tap
    // reaches half of one either side, so half of that is the margin -- and
    // taking it from the coarser of the two levels a trilinear read touches
    // covers both. At mip zero it is exactly half a texel, which is the centre
    // of the tile's own edge texel: no interpolation, and nothing moved.
    let margin = 0.5 * exp2(ceil(max(lod, 0.0)));
    let low = vec2<f32>(hit.origin) + vec2<f32>(margin, margin);
    let high = vec2<f32>(hit.origin + vec2<u32>(tile, tile)) - vec2<f32>(margin, margin);
    let centre = vec2<f32>(hit.texel) + vec2<f32>(0.5, 0.5) + vec2<f32>(hit.origin);
    return clamp(centre, min(low, high), max(low, high)) / size;
}

// One source, one uv, one texel of it, at a mip level the caller computed.
//
// A page nothing resident covers reads as transparent black rather than as
// whatever happens to be in the slot, so a caller can tell a hole from a
// colour.
//
// `textureSampleLevel` rather than `textureSample`: an explicit level needs no
// uniform control flow, which matters because the branch above it is on whether
// a tile is resident and that is not uniform across a quad.
fn corvid_tile_sample_level(source: u32, uv: vec2<f32>, lod: f32) -> vec4<f32> {
    let hit = corvid_tile_lookup(source, uv);
    if !hit.present {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return textureSampleLevel(
        corvid_tile_atlas,
        corvid_tile_sampler,
        corvid_tile_point(hit, lod),
        i32(hit.layer),
        lod,
    );
}

// The same, at the sharpest mip there is.
fn corvid_tile_sample(source: u32, uv: vec2<f32>) -> vec4<f32> {
    return corvid_tile_sample_level(source, uv, 0.0);
}
