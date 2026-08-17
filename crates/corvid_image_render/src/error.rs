//! Everything the device half refuses to do, and the reason each refusal
//! exists.

use corvid_image::{ConfigError, Extent, PixelFormat, TileKey, TileSlot};

/// A plan that cannot be carried out on this device.
///
/// Every variant is a disagreement between three things that have to agree: the
/// [`TileConfig`](corvid_image::TileConfig) the plan was made under, the
/// [`wgpu::Limits`] the device was opened with, and the tile that actually
/// arrived. None of them is recoverable by retrying, which is why each names
/// both sides of the disagreement rather than saying that one happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum CacheError {
    /// The tile configuration is a shape the packed table entry has no bits
    /// for.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// One tile is wider than the largest texture this device will allocate.
    ///
    /// A cache whose slot is bigger than a texture is not a cache with fewer
    /// slots; it is a cache with none, which is why this is an error rather
    /// than a smaller number.
    #[error("a tile of {tile} texels a side does not fit a device whose largest texture is {max}")]
    TileTooBig {
        /// [`TileConfig::tile_size`](corvid_image::TileConfig::tile_size).
        tile: u32,
        /// [`wgpu::Limits::max_texture_dimension_2d`].
        max: u32,
    },
    /// `wgpu` has no eight-bit texture format for this pixel format.
    ///
    /// There is no one- or two-channel sRGB texture in any graphics API, so a
    /// mask or a coverage map declared as sRGB has nowhere to live. Declare it
    /// [`ColorSpace::Linear`](corvid_image::ColorSpace::Linear), which is what
    /// it is.
    #[error("no eight-bit texture format holds {0}")]
    NoTextureFormat(PixelFormat),
    /// The plan named a slot this cache does not have.
    ///
    /// A plan made against a larger budget than the cache was built with, which
    /// is a caller holding two different numbers rather than a transient
    /// condition.
    #[error("{slot} is past the {capacity} slots this cache has")]
    NoSuchSlot {
        /// The slot the plan named.
        slot: TileSlot,
        /// How many this cache has.
        capacity: u32,
    },
    /// A tile was uploaded to a slot that still holds a different one.
    ///
    /// The plan's evictions are performed before its uploads for exactly this
    /// reason: the planner hands a freed slot straight back out, so an upload
    /// reaching one that was never freed would overwrite a tile the table still
    /// points at.
    #[error("{slot} still holds {held}, so the plan's eviction of it was never performed")]
    SlotOccupied {
        /// The slot that was written to.
        slot: TileSlot,
        /// What is still in it.
        held: TileKey,
    },
    /// The tile's pixel format is not the one the cache was built for.
    ///
    /// One format for the whole cache rather than one per source, because a
    /// slot has to be able to hold any tile that lands in it.
    #[error("a tile of {given} was uploaded to a cache of {wanted}")]
    Format {
        /// What the cache holds.
        wanted: PixelFormat,
        /// What arrived.
        given: PixelFormat,
    },
    /// The tile is larger than one slot.
    ///
    /// A tile at the right or bottom edge of a source is *smaller* than a slot
    /// and that is normal; larger is a decoder that read the wrong region.
    #[error("a tile of {extent} does not fit a {tile}-texel slot")]
    TileTooLarge {
        /// The size that arrived.
        extent: Extent,
        /// The slot's side, in texels.
        tile: u32,
    },
    /// The table was built for a different tile size than the cache.
    ///
    /// The table's arithmetic and the atlas's layout both divide by the tile
    /// size, so two of them is every sample landing somewhere else.
    #[error("a table for {given}-texel tiles cannot be read by a cache of {wanted}-texel ones")]
    TableTileSize {
        /// The cache's tile side.
        wanted: u32,
        /// The table's.
        given: u32,
    },
    /// The table has more layers than the shader's source array has room for.
    #[error("a table of {given} sources is past the {wanted} a parameter block holds")]
    TooManySources {
        /// How many the parameter block holds.
        wanted: u32,
        /// How many the table has.
        given: u32,
    },
    /// The table is wider in pages than a texture on this device reaches.
    ///
    /// Only a configuration with a very small tile over a very large image gets
    /// here: 131072 texels over 16-texel tiles is 8192 pages a side, which is
    /// the largest texture many devices have.
    #[error("a table of {given} pages a side is past the {max} a texture on this device reaches")]
    TableTooWide {
        /// The table's side, in pages.
        given: u32,
        /// [`wgpu::Limits::max_texture_dimension_2d`].
        max: u32,
    },
}
