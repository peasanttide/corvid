//! The numbers the whole plan is shaped by, and the check that they fit.

use crate::ConfigError;
use crate::tile::SLOT_BITS;

/// The VRAM a machine at the minimum specification has, in bytes.
///
/// Four gibibytes, and it is the *card* rather than the budget: everything else
/// a frame needs comes out of the same four, so a caller carves a share with
/// [`VramBudget::share`](crate::VramBudget::share) rather than handing the
/// whole of it to the tile cache.
pub const MIN_SPEC_VRAM: u64 = 4 << 30;

/// The side of a tile, in texels.
pub const TILE_SIZE: u32 = 256;

/// How many tiles the minimum specification is asked to hold resident.
pub const MAX_NUM_TILES: u32 = 2048;

/// How many sources one plan may cover, which is what makes a source id a byte.
pub const MAX_NUM_MAPS: u32 = 255;

/// The largest image side a plan will address, in texels.
///
/// Larger than any plate in the archive on purpose. The point of the limit is
/// not that 131072 is enough; it is that a number exists at all, so that a
/// header claiming four billion texels a side is refused at registration
/// instead of overflowing a page index halfway through a frame.
pub const MAX_IMAGE_SIZE: u32 = 1 << 17;

/// The four numbers a tile plan is shaped by.
///
/// [`MIN_SPEC`](Self::MIN_SPEC) is the minimum specification from the design
/// and is also [`Default`], but nothing here is baked: a machine with more
/// memory raises [`max_tiles`](Self::max_tiles), and a build that only ever
/// shows small pictures lowers [`max_image_size`](Self::max_image_size) and
/// gets a smaller lookup table for it.
///
/// ```
/// use corvid_image::{MAX_NUM_TILES, TILE_SIZE, TileConfig};
///
/// let config = TileConfig::MIN_SPEC;
/// assert_eq!(config.tile_size, TILE_SIZE);
/// assert_eq!(config.max_tiles, MAX_NUM_TILES);
/// config.validate()?;
/// # Ok::<(), corvid_image::ConfigError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct TileConfig {
    /// The side of one tile, in texels. A power of two in 16..=16384.
    pub tile_size: u32,
    /// How many tiles may be resident at once, across every source.
    pub max_tiles: u32,
    /// How many sources one plan may cover.
    pub max_sources: u32,
    /// The largest image side a source may have. A power of two.
    pub max_image_size: u32,
}

impl TileConfig {
    /// The design's minimum specification: 256-texel tiles, 2048 of them, 255
    /// sources, and a 131072-texel ceiling on a side.
    pub const MIN_SPEC: Self = Self {
        tile_size: TILE_SIZE,
        max_tiles: MAX_NUM_TILES,
        max_sources: MAX_NUM_MAPS,
        max_image_size: MAX_IMAGE_SIZE,
    };

    /// The coarsest zoom any plan will reach, whatever the configuration.
    ///
    /// Eight, because a table entry spends eight bits on the offset of a page
    /// inside its tile and a tile at level `L` spans `2^L` pages. That is the
    /// price of the entry being one 32-bit word, and it is the reason a source
    /// wider than `tile_size << 8` is a small grid of tiles at its coarsest
    /// level rather than a single one. With the default 256-texel tile that
    /// threshold is 65536 texels, so the very largest plates bottom out at a
    /// two-by-two or four-by-four root instead of a one-by-one.
    pub const MAX_LEVEL: u8 = crate::tile::MAX_LEVEL;

    /// Whether these four numbers can be honoured.
    ///
    /// # Errors
    ///
    /// One [`ConfigError`] per way they cannot: see that enum, whose every
    /// variant is a shape [`TileEntry`](crate::TileEntry) has no bits for.
    pub const fn validate(&self) -> Result<(), ConfigError> {
        if !self.tile_size.is_power_of_two() || self.tile_size < 16 || self.tile_size > 16384 {
            return Err(ConfigError::TileSize(self.tile_size));
        }
        if !self.max_image_size.is_power_of_two() || self.max_image_size < self.tile_size {
            return Err(ConfigError::MaxImageSize {
                size: self.max_image_size,
                tile: self.tile_size,
            });
        }
        // The all-ones slot is "no tile here", so a budget of `1 << SLOT_BITS`
        // would leave nothing to say that with.
        if self.max_tiles == 0 || self.max_tiles >= (1 << SLOT_BITS) {
            return Err(ConfigError::TileCount {
                tiles: self.max_tiles,
                bits: SLOT_BITS,
            });
        }
        if self.max_sources == 0 || self.max_sources > MAX_NUM_MAPS {
            return Err(ConfigError::SourceCount(self.max_sources));
        }
        Ok(())
    }

    /// How many tiles of `tile_size` texels it takes to cover `texels`.
    ///
    /// This is the design's `1 + (size - 1) / tile_size`, written as the shift
    /// it is: `tile_size` is a power of two, so the shader does the same sum
    /// without a division.
    #[must_use]
    pub const fn pages_across(&self, texels: u32) -> u32 {
        if texels == 0 {
            return 0;
        }
        let shift = self.tile_size.trailing_zeros();
        ((texels - 1) >> shift) + 1
    }

    /// How many bytes one tile of `format` weighs on the device.
    #[must_use]
    pub const fn tile_bytes(&self, format: crate::PixelFormat) -> u64 {
        self.tile_size as u64 * self.tile_size as u64 * format.bytes_per_texel() as u64
    }
}

impl Default for TileConfig {
    fn default() -> Self {
        Self::MIN_SPEC
    }
}

/// How much device memory the tile cache may occupy.
///
/// Bytes rather than tiles, because the cost of a tile depends on its format
/// and the caller knows how much of the card is already spoken for. The plan
/// turns it back into a tile count with
/// [`capacity`](Self::capacity), and takes the smaller of that and
/// [`TileConfig::max_tiles`].
///
/// ```
/// use corvid_image::{PixelFormat, TileConfig, VramBudget};
///
/// let config = TileConfig::MIN_SPEC;
/// // A quarter of a minimum-spec card, in four-channel tiles.
/// let budget = VramBudget::MIN_SPEC.share(25);
/// assert_eq!(budget.capacity(&config, PixelFormat::SRGBA8), config.max_tiles);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct VramBudget {
    /// The bytes on offer.
    pub bytes: u64,
}

impl VramBudget {
    /// The whole of a minimum-specification card, which is [`MIN_SPEC_VRAM`].
    pub const MIN_SPEC: Self = Self::new(MIN_SPEC_VRAM);

    /// A budget of so many bytes.
    #[must_use]
    pub const fn new(bytes: u64) -> Self {
        Self { bytes }
    }

    /// The given whole percentage of this budget, saturating at 100.
    #[must_use]
    pub const fn share(self, percent: u32) -> Self {
        let percent = if percent > 100 { 100 } else { percent } as u64;
        Self::new(self.bytes / 100 * percent)
    }

    /// How many tiles of `format` fit, capped by
    /// [`TileConfig::max_tiles`].
    ///
    /// The cap is not a formality. A card with room for sixteen thousand tiles
    /// still gets `max_tiles`, because the lookup table's slot field is that
    /// many bits wide and the minimum specification is the number the shader
    /// was written against.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the quotient is compared against max_tiles, itself a u32, before the cast, so the cast is only reached on a value smaller than one"
    )]
    pub const fn capacity(self, config: &TileConfig, format: crate::PixelFormat) -> u32 {
        let each = config.tile_bytes(format);
        if each == 0 {
            return 0;
        }
        let fits = self.bytes / each;
        if fits > config.max_tiles as u64 {
            config.max_tiles
        } else {
            fits as u32
        }
    }
}
