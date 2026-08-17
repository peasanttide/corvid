//! What the planner knows about a picture it has never seen the pixels of.

use crate::{Extent, PixelFormat, TileConfig, TileError, TileKey, tile::SourceId};

/// A registered picture: how big it is, what a texel of it is, and how deep its
/// tile pyramid goes.
///
/// This is the design's "source map", and it is deliberately not an [`Image`].
/// A plate is registered from its header, planned against, and streamed tile by
/// tile; the whole point is that its pixels never all exist at once on this
/// side of the fence.
///
/// [`Image`]: crate::Image
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Source {
    extent: Extent,
    format: PixelFormat,
    top_level: u8,
}

impl Source {
    /// Register a picture of this size and format under `config`.
    ///
    /// # Errors
    ///
    /// [`TileError::Empty`] for a picture with no texels, and
    /// [`TileError::TooLarge`] for one wider or taller than
    /// [`TileConfig::max_image_size`]. The second is the one that matters: a
    /// plate too big for the page index is refused here rather than clipped,
    /// because a clipped map is a map with a wrong eastern edge and nothing
    /// downstream can tell.
    pub fn new(
        config: &TileConfig,
        extent: Extent,
        format: PixelFormat,
    ) -> Result<Self, TileError> {
        if extent.is_empty() {
            return Err(TileError::Empty { extent });
        }
        if extent.width > config.max_image_size || extent.height > config.max_image_size {
            return Err(TileError::TooLarge {
                extent,
                max: config.max_image_size,
            });
        }
        let mut top_level = 0u8;
        while top_level < TileConfig::MAX_LEVEL {
            let [across, down] = tiles_at(config, extent, u32::from(top_level));
            if across <= 1 && down <= 1 {
                break;
            }
            top_level += 1;
        }
        Ok(Self {
            extent,
            format,
            top_level,
        })
    }

    /// How big the picture is, at level zero.
    #[must_use]
    pub const fn extent(&self) -> Extent {
        self.extent
    }

    /// What a texel of it is made of.
    #[must_use]
    pub const fn format(&self) -> PixelFormat {
        self.format
    }

    /// The coarsest level this source's pyramid reaches.
    ///
    /// The level at which the whole picture is one tile, or
    /// [`TileConfig::MAX_LEVEL`] if the picture is too big to get there. See
    /// that constant for why the ceiling exists.
    #[must_use]
    pub const fn top_level(&self) -> u8 {
        self.top_level
    }

    /// How many level-zero pages the picture is, across and down.
    ///
    /// This is what the lookup table is indexed by, so it is also what sets the
    /// table's side.
    #[must_use]
    pub const fn pages(&self, config: &TileConfig) -> [u32; 2] {
        [
            config.pages_across(self.extent.width),
            config.pages_across(self.extent.height),
        ]
    }

    /// How many tiles the picture is at `level`, across and down.
    #[must_use]
    pub const fn tiles_at(&self, config: &TileConfig, level: u32) -> [u32; 2] {
        tiles_at(config, self.extent, level)
    }

    /// Whether `key` names a tile this source actually has.
    #[must_use]
    pub fn contains(&self, config: &TileConfig, key: TileKey) -> bool {
        if u32::from(key.level) > u32::from(self.top_level) {
            return false;
        }
        let [x, y] = self.tiles_at(config, u32::from(key.level));
        u32::from(key.x) < x && u32::from(key.y) < y
    }
}

const fn tiles_at(config: &TileConfig, extent: Extent, level: u32) -> [u32; 2] {
    // A tile at `level` covers `tile_size << level` texels a side. Shifting the
    // extent down first and the tile size not at all keeps the product inside a
    // `u32` for a 131072-texel plate at level 8, which multiplying would not.
    let side = if level >= u32::BITS {
        u32::MAX
    } else {
        match config.tile_size.checked_shl(level) {
            Some(side) => side,
            None => u32::MAX,
        }
    };
    let across = 1 + (extent.width - 1) / side;
    let down = 1 + (extent.height - 1) / side;
    [across, down]
}

/// The rows of the design's source map: every picture the plan covers, in
/// registration order.
///
/// A source's id is its position here, which is what lets the lookup table be a
/// flat array of layers and the shader index it with one multiply.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Sources {
    rows: alloc::vec::Vec<Source>,
}

impl Sources {
    /// No sources at all.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            rows: alloc::vec::Vec::new(),
        }
    }

    /// Add a picture, answering the id it was given.
    ///
    /// # Errors
    ///
    /// Whatever [`Source::new`] refuses, plus [`TileError::TooManySources`]
    /// once [`TileConfig::max_sources`] are registered.
    pub fn push(
        &mut self,
        config: &TileConfig,
        extent: Extent,
        format: PixelFormat,
    ) -> Result<SourceId, TileError> {
        // The `u8` conversion is the same check as the length one, spelled
        // where it cannot be forgotten: `validate` caps `max_sources` at 255,
        // so a row index that will not fit a byte cannot be reached -- and if
        // one ever were, the honest answer is the error rather than a source
        // that silently becomes source zero.
        let full = self.rows.len() >= config.max_sources as usize;
        let id = u8::try_from(self.rows.len()).ok().filter(|_| !full);
        let Some(id) = id else {
            return Err(TileError::TooManySources(config.max_sources));
        };
        self.rows.push(Source::new(config, extent, format)?);
        Ok(SourceId(id))
    }

    /// The picture with this id, or `None` if nothing was registered under it.
    #[must_use]
    pub fn get(&self, id: SourceId) -> Option<&Source> {
        self.rows.get(id.index())
    }

    /// How many pictures are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether none are.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Every picture, in id order.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "push refuses a row past max_sources, which validate caps at 255, so every index here fits a byte"
    )]
    pub fn iter(&self) -> impl Iterator<Item = (SourceId, &Source)> {
        self.rows
            .iter()
            .enumerate()
            .map(|(index, source)| (SourceId(index as u8), source))
    }
}
