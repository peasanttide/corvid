//! The uniform block the shader reads before it touches either texture.
//!
//! The seam is layout: this is the only Rust in the crate whose field order is
//! load-bearing, because `tiles.wgsl` declares the same struct and a device
//! reads whichever one is wrong without complaining.

use corvid_image::{SourceId, TileTable};

use crate::atlas::Atlas;
use crate::shader::MAX_SOURCES;

/// How many entries the source array has, as the `usize` an array length is.
const SOURCES: usize = MAX_SOURCES as usize;

/// `CorvidTileParams` from `tiles.wgsl`, field for field.
///
/// Boxed by its one caller rather than passed around: four kilobytes is a lot
/// to move for a struct that is written to a buffer and dropped.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Params {
    /// Tile side, its log, the table's side in pages, and how many sources.
    shape: [u32; 4],
    /// The two atlas shifts, then one layer's size in texels.
    atlas: [u32; 4],
    /// Each source's extent in `xy`. `zw` is the padding a uniform array's
    /// sixteen-byte stride forces, and the shader never reads it.
    sources: [[u32; 4]; SOURCES],
}

impl Params {
    /// How many bytes the uniform buffer is.
    pub(crate) const BYTES: u64 = size_of::<Self>() as u64;

    /// The block describing this table in this atlas.
    ///
    /// A source past [`MAX_SOURCES`] is dropped rather than wrapped: the shader
    /// answers "nothing here" for a source it has no extent for, which is the
    /// same thing a table with no layer for it answers, and
    /// `TileConfig::validate` is what stops either from being reachable.
    pub(crate) fn new(table: &TileTable, atlas: &Atlas) -> Self {
        let (across_shift, layer_shift) = atlas.shifts();
        let (layer_width, layer_height) = atlas.layer_extent();
        let mut sources = [[0u32; 4]; SOURCES];
        for (index, row) in sources.iter_mut().enumerate() {
            let id = u8::try_from(index).map(SourceId);
            let Some(extent) = id.ok().and_then(|id| table.extent(id)) else {
                break;
            };
            *row = [extent.width, extent.height, 0, 0];
        }
        Self {
            shape: [
                table.tile_size(),
                table.tile_size().trailing_zeros(),
                table.side(),
                table.layers(),
            ],
            atlas: [across_shift, layer_shift, layer_width, layer_height],
            sources,
        }
    }
}
