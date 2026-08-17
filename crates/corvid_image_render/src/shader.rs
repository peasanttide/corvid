//! The WGSL a caller includes, and the group it declares.
//!
//! The seam is agreement. The layout of a [`TileEntry`](corvid_image::TileEntry)
//! and the code that reads it have to match, and splitting them across two
//! crates is how they stop matching -- so the shader snippet ships here, beside
//! the Rust that fills the buffers it reads.

use alloc::format;
use alloc::string::{String, ToString as _};

/// How many sources the parameter block has room for.
///
/// Fixed at 256 rather than configured, because a uniform array's length is a
/// compile-time constant in WGSL. It is enough for every configuration there
/// is: [`MAX_NUM_MAPS`](corvid_image::MAX_NUM_MAPS) is 255 and
/// [`TileConfig::validate`](corvid_image::TileConfig::validate) refuses more.
pub const MAX_SOURCES: u32 = 256;

/// The bind group [`WGSL`] declares its four bindings in.
///
/// One rather than zero, because zero is where a game usually puts its own
/// per-frame uniforms and the tile bindings change far less often than those
/// do. Use [`wgsl_at`] for a shader that wants them somewhere else.
pub const GROUP: u32 = 1;

/// The WGSL that resolves a source id and a uv through the lookup table.
///
/// Concatenate it with a shader of your own and compile the result; it declares
/// no entry point, because a shader has one set of those and they are yours. It
/// declares [`GROUP`], which [`TileCache::layout`](crate::TileCache::layout)
/// describes and [`TileCache::bind_group`](crate::TileCache::bind_group) fills,
/// and four functions over it:
///
/// `corvid_tile_lookup(source, uv)` answers a `CorvidTileHit` -- whether
/// anything is resident, the zoom serving the page, and the texel inside the
/// tile, which is [`TileSample`](corvid_image::TileSample) to the texel.
/// `corvid_tile_point(hit, lod)` turns a hit into the coordinate a sampler
/// takes. `corvid_tile_sample(source, uv)` and
/// `corvid_tile_sample_level(source, uv, lod)` are the two together.
///
/// ```
/// use corvid_image_render::{GROUP, WGSL};
///
/// // It is source text, so a game builds its module out of both halves.
/// let mine = format!("{WGSL}\n@fragment fn main() -> @location(0) vec4<f32> {{
///     return corvid_tile_sample(0u, vec2<f32>(0.5, 0.5));
/// }}");
/// assert!(mine.contains(&format!("@group({GROUP})")));
/// ```
pub const WGSL: &str = include_str!("tiles.wgsl");

/// [`WGSL`] with its bindings moved to another bind group.
///
/// A shader with three groups of its own wants the tile bindings in the fourth,
/// and a WGSL `@group` takes a literal rather than a constant -- so the group is
/// substituted in the text. That is a string replacement and it is exactly as
/// fragile as it sounds; what makes it safe is that the only thing in
/// [`WGSL`] spelled `@group(1)` is the four bindings this crate wrote.
///
/// ```
/// use corvid_image_render::{GROUP, wgsl_at};
///
/// let moved = wgsl_at(3);
/// assert_eq!(moved.matches("@group(3)").count(), 4);
/// assert!(!moved.contains(&format!("@group({GROUP}) @binding")));
/// ```
#[must_use]
pub fn wgsl_at(group: u32) -> String {
    if group == GROUP {
        return WGSL.to_string();
    }
    WGSL.replace(&format!("@group({GROUP})"), &format!("@group({group})"))
}

/// The shader the mip chain is built with, which is this crate's own and not a
/// caller's business.
pub(crate) const MIPS_WGSL: &str = include_str!("mips.wgsl");
