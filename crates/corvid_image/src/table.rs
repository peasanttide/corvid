//! The lookup table a fragment shader reads: uv in, tile out.

use alloc::vec;
use alloc::vec::Vec;

use crate::{Extent, Residency, SourceId, Sources, TileConfig, TileEntry, TileKey, TileSlot};

/// What sampling a source at a uv resolves to.
///
/// The CPU mirror of what the fragment shader computes from one
/// [`TileEntry`]. It exists so the arithmetic can be tested against a
/// hand-worked answer on a machine with no GPU in it, which is the only way a
/// shader's addressing is ever actually checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileSample {
    /// Where in the device's tile array to sample.
    pub slot: TileSlot,
    /// The zoom of the tile in that slot, which the caller needs to know
    /// because it is not necessarily the zoom that was asked for.
    pub level: u8,
    /// The texel inside that tile, in `0..tile_size`.
    pub texel: [u32; 2],
}

/// The design's tile map: for every page of every source, which tile serves it.
///
/// One layer per source, each `side` pages square, where `side` is the widest
/// registered source in level-zero pages. Index it as
/// `layer * side * side + page_y * side + page_x` -- one multiply and one add,
/// which is what a fragment shader can afford per sample.
///
/// The layers are uniform because a texture array's layers are, and that is the
/// table's one real cost: a plan holding many maximum-size sources pays
/// `side^2` words for each of them whether or not the source is that big. The
/// side is derived from the sources actually registered rather than from
/// [`TileConfig::max_image_size`] for exactly that reason -- deriving it from
/// the configured ceiling would make every table the size of the largest table
/// anyone could ever ask for.
///
/// A table describes the residency a plan *produces*, not the one it started
/// from. Upload the tiles and the table together, or a frame samples slots
/// whose contents have not landed yet.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TileTable {
    side: u32,
    tile_shift: u32,
    tile_size: u32,
    extents: Vec<Extent>,
    words: Vec<i32>,
}

impl TileTable {
    /// The table for a residency, serving each source at the finest level in
    /// `desired` that anything resident can cover.
    ///
    /// `desired[i]` is the finest zoom wanted for source `i`; a source past the
    /// end of the slice, or one asking for a zoom finer than it has, is served
    /// from the top of its own pyramid. Every page then walks coarser from
    /// there until it finds a resident tile, so a page whose detail has not
    /// arrived is blurry rather than absent, and one with nothing resident at
    /// all is [`TileEntry::ABSENT`].
    #[must_use]
    pub fn build(
        config: &TileConfig,
        sources: &Sources,
        resident: &Residency,
        desired: &[u8],
    ) -> Self {
        let side = sources
            .iter()
            .map(|(_, source)| {
                let [x, y] = source.pages(config);
                x.max(y)
            })
            .max()
            .unwrap_or(0);
        let layers = sources.len();
        let extents = sources.iter().map(|(_, source)| source.extent()).collect();
        let mut words = vec![TileEntry::ABSENT.bits(); side as usize * side as usize * layers];

        for (id, source) in sources.iter() {
            let top = source.top_level();
            let want = desired.get(id.index()).copied().unwrap_or(top).min(top);
            let [pages_x, pages_y] = source.pages(config);
            let base = id.index() * side as usize * side as usize;
            for page_y in 0..pages_y {
                for page_x in 0..pages_x {
                    let key = TileKey::new(
                        id,
                        want,
                        as_u16(page_x >> u32::from(want)),
                        as_u16(page_y >> u32::from(want)),
                    );
                    let Some((served, slot)) = resident.nearest_at_or_coarser(key, top) else {
                        continue;
                    };
                    // The offset of this level-zero page inside the tile that
                    // ended up serving it. `at_level`/`offset_in` are written
                    // in terms of a key's own level, so the page is spelled as
                    // a level-zero key to ask.
                    let page_key = TileKey::new(id, 0, as_u16(page_x), as_u16(page_y));
                    let [off_u, off_v] = page_key.offset_in(served.level);
                    let index = base + page_y as usize * side as usize + page_x as usize;
                    if let Some(word) = words.get_mut(index) {
                        *word = TileEntry::new(slot, served.level, [off_u, off_v]).bits();
                    }
                }
            }
        }

        Self {
            side,
            tile_shift: config.tile_size.trailing_zeros(),
            tile_size: config.tile_size,
            extents,
            words,
        }
    }

    /// How many pages a layer is on a side.
    #[must_use]
    pub const fn side(&self) -> u32 {
        self.side
    }

    /// How many layers, which is how many sources.
    #[must_use]
    pub fn layers(&self) -> u32 {
        u32::try_from(self.extents.len()).unwrap_or(u32::MAX)
    }

    /// The tile side the table was built for, in texels.
    #[must_use]
    pub const fn tile_size(&self) -> u32 {
        self.tile_size
    }

    /// The whole table, as the device reads it.
    ///
    /// `i32` because the design's table is a table of `i32`. Nothing does
    /// arithmetic on the sign; the shader masks out three fields and uses them.
    #[must_use]
    pub fn words(&self) -> &[i32] {
        &self.words
    }

    /// The size the table believes a source is.
    #[must_use]
    pub fn extent(&self, source: SourceId) -> Option<Extent> {
        self.extents.get(source.index()).copied()
    }

    /// The entry for one page of one source, or [`TileEntry::ABSENT`] for a
    /// page outside the table.
    #[must_use]
    pub fn entry(&self, source: SourceId, page_x: u32, page_y: u32) -> TileEntry {
        if page_x >= self.side || page_y >= self.side {
            return TileEntry::ABSENT;
        }
        let index = source.index() * self.side as usize * self.side as usize
            + page_y as usize * self.side as usize
            + page_x as usize;
        self.words
            .get(index)
            .copied()
            .map_or(TileEntry::ABSENT, TileEntry::from_bits)
    }

    /// Where to sample for a uv on a source, or `None` if nothing covers it.
    ///
    /// This is the fragment shader, in Rust and to the texel. A uv becomes a
    /// level-zero texel, the texel becomes a page by a shift, the page becomes
    /// an entry by one indexed load, and the entry becomes a slot and a texel
    /// inside it by two masks and a shift:
    ///
    /// ```text
    /// texel = uv * extent
    /// page  = texel >> log2(tile_size)
    /// entry = words[layer * side * side + page.y * side + page.x]
    /// inner = (entry.offset * tile_size + (texel & (tile_size - 1))) >> entry.level
    /// ```
    ///
    /// The shift by `entry.level` at the end is the whole of what makes a
    /// coarse fallback work: a tile four levels up covers sixteen times the
    /// ground, so the texel inside it is sixteen times closer to its origin.
    ///
    /// A uv outside `[0, 1)` is clamped to the edge texel, which is what a
    /// clamped sampler does and what stops a rounding error at the seam of two
    /// plates from reading the wrong page.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "an extent is at most 131072, which an f32 holds exactly; the product is then clamped to it, and a float cast in Rust saturates rather than wrapping, so a NaN lands on texel zero"
    )]
    pub fn resolve(&self, source: SourceId, uv: [f32; 2]) -> Option<TileSample> {
        let extent = self.extent(source)?;
        let texel_x = ((uv[0] * extent.width as f32) as u32).min(extent.width - 1);
        let texel_y = ((uv[1] * extent.height as f32) as u32).min(extent.height - 1);
        let entry = self.entry(
            source,
            texel_x >> self.tile_shift,
            texel_y >> self.tile_shift,
        );
        let slot = entry.slot()?;
        let level = u32::from(entry.level());
        let [off_u, off_v] = entry.offset();
        let mask = self.tile_size - 1;
        let inner_x = (u32::from(off_u) * self.tile_size + (texel_x & mask)) >> level;
        let inner_y = (u32::from(off_v) * self.tile_size + (texel_y & mask)) >> level;
        Some(TileSample {
            slot,
            level: entry.level(),
            texel: [inner_x, inner_y],
        })
    }
}

/// A page index as the `u16` a [`TileKey`] holds.
///
/// The largest configuration is 131072 texels over 16-texel tiles, which is
/// 8192 pages: inside a `u16` with three bits to spare, and
/// [`TileConfig::validate`] is what guarantees it.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the comparison above is the range check the cast needs"
)]
const fn as_u16(page: u32) -> u16 {
    if page > u16::MAX as u32 {
        u16::MAX
    } else {
        page as u16
    }
}
