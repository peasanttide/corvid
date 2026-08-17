//! The three things a tile is named by, and the word the shader reads.

use core::fmt;

/// How many bits of a table entry name a slot in the tile array.
///
/// Twelve, so 4095 tiles are addressable and the 4096th value is the sentinel
/// for "nothing here". The minimum specification asks for 2048.
pub(crate) const SLOT_BITS: u32 = 12;

/// How many bits of a table entry hold the zoom.
pub(crate) const LEVEL_BITS: u32 = 4;

/// How many bits of a table entry hold each half of the page offset.
///
/// This is the field that caps the pyramid: a tile at level `L` spans `2^L`
/// pages, so eight bits stop the plan at level eight. See
/// [`TileConfig::MAX_LEVEL`](crate::TileConfig::MAX_LEVEL).
pub(crate) const OFFSET_BITS: u32 = 8;

/// The coarsest zoom the packing can express.
///
/// A tile at level `L` spans `2^L` pages, so the offset field is what caps it:
/// eight bits of offset stop the pyramid at level eight. The assertion below is
/// the one that would fire if either field were resized without the other.
pub(crate) const MAX_LEVEL: u8 = 8;

const _: () = assert!(
    (1u32 << MAX_LEVEL) - 1 < (1u32 << OFFSET_BITS) && MAX_LEVEL < (1u8 << LEVEL_BITS),
    "the largest offset a level must express has to fit the offset field, and the level has to fit its own"
);

const SLOT_MASK: u32 = (1 << SLOT_BITS) - 1;
const LEVEL_MASK: u32 = (1 << LEVEL_BITS) - 1;
const OFFSET_MASK: u32 = (1 << OFFSET_BITS) - 1;

const LEVEL_SHIFT: u32 = SLOT_BITS;
const OFFSET_U_SHIFT: u32 = SLOT_BITS + LEVEL_BITS;
const OFFSET_V_SHIFT: u32 = SLOT_BITS + LEVEL_BITS + OFFSET_BITS;

/// Which source a tile belongs to.
///
/// A byte, which is why [`MAX_NUM_MAPS`](crate::MAX_NUM_MAPS) is 255: the id is
/// a layer index in the lookup table and a row in the source table, and both
/// want it small.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct SourceId(pub u8);

impl SourceId {
    /// The id as an index into a table.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "source {}", self.0)
    }
}

/// Where a tile lives in the device's tile array.
///
/// A slot is a *position*, not an identity: the same slot holds a different
/// tile after an eviction. Only [`TileKey`] identifies a tile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileSlot(pub u16);

impl TileSlot {
    /// The slot as an index into the tile array.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for TileSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "slot {}", self.0)
    }
}

/// One tile of one source's pyramid, named by source, zoom and position.
///
/// The field order is the sort order, and the sort order is load-bearing: a
/// plan is only reproducible if every collection in it has a total order that
/// does not depend on where anything was allocated.
///
/// Coordinates are in tiles at their own level, so the tile covering page
/// `(px, py)` at level `L` is at `(px >> L, py >> L)`. That shift is the whole
/// of the relationship between levels and is why a coarser fallback is found by
/// shifting rather than by searching.
///
/// ```
/// use corvid_image::{SourceId, TileKey};
///
/// let fine = TileKey::new(SourceId(0), 0, 37, 11);
/// assert_eq!(fine.at_level(3), TileKey::new(SourceId(0), 3, 4, 1));
/// // The offset of the fine page inside that coarse tile.
/// assert_eq!(fine.offset_in(3), [5, 3]);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileKey {
    /// Which source.
    pub source: SourceId,
    /// The power-of-two zoom: level `L` covers `2^L` level-zero pages a side.
    pub level: u8,
    /// Column, in tiles at this level.
    pub x: u16,
    /// Row, in tiles at this level.
    pub y: u16,
}

impl TileKey {
    /// A key from its four parts.
    #[must_use]
    pub const fn new(source: SourceId, level: u8, x: u16, y: u16) -> Self {
        Self {
            source,
            level,
            x,
            y,
        }
    }

    /// The tile at `level` covering the same texels as this one.
    ///
    /// Only meaningful for a coarser `level` than this key's own. Going the
    /// other way there are `4^d` tiles and no single answer, so this shifts
    /// down and answers the one containing this key's top-left corner.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "shifting a u16 right can only shrink it, so the u32 the shift is done in is back inside a u16 before the cast"
    )]
    pub const fn at_level(self, level: u8) -> Self {
        let shift = level.saturating_sub(self.level) as u32;
        if shift >= u16::BITS {
            return Self::new(self.source, level, 0, 0);
        }
        Self::new(
            self.source,
            level,
            (self.x as u32 >> shift) as u16,
            (self.y as u32 >> shift) as u16,
        )
    }

    /// Where this tile sits inside the coarser tile at `level`, in units of
    /// this key's own tiles.
    ///
    /// This is the `offset` a [`TileEntry`] carries, and it is a mask rather
    /// than a subtraction: the coarse tile's origin is `(x >> d) << d`, so the
    /// remainder is `x & ((1 << d) - 1)`.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "masking a u16 can only clear bits, so the u32 the mask is done in is back inside a u16 before the cast"
    )]
    pub const fn offset_in(self, level: u8) -> [u16; 2] {
        let shift = level.saturating_sub(self.level) as u32;
        if shift >= u16::BITS {
            return [self.x, self.y];
        }
        let mask = (1u32 << shift) - 1;
        [(self.x as u32 & mask) as u16, (self.y as u32 & mask) as u16]
    }
}

impl fmt::Display for TileKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} tile ({}, {}) at zoom {}",
            self.source, self.x, self.y, self.level
        )
    }
}

/// One word of the lookup table: which tile, where in it, and at what zoom.
///
/// Thirty-two bits, laid out low to high as twelve for the slot, four for the
/// level, and eight each for the two halves of the page offset. A fragment
/// shader reads the word, masks out three fields and has everything it needs;
/// there is no second load and no dependent branch.
///
/// The offset is derivable -- it is `page & ((1 << level) - 1)` -- and it is
/// stored anyway. That is deliberate: it keeps the layout of the pyramid a
/// thing only the planner knows, so the level a page is served at can stop
/// being a pure function of the page index the day a plan wants to serve two
/// halves of one source at different zooms.
///
/// ```
/// use corvid_image::{TileEntry, TileSlot};
///
/// let entry = TileEntry::new(TileSlot(1337), 3, [5, 3]);
/// assert_eq!(entry.slot(), Some(TileSlot(1337)));
/// assert_eq!(entry.level(), 3);
/// assert_eq!(entry.offset(), [5, 3]);
///
/// // A round trip through the word the shader actually reads.
/// assert_eq!(TileEntry::from_bits(entry.bits()), entry);
/// assert_eq!(TileEntry::ABSENT.slot(), None);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileEntry(u32);

impl TileEntry {
    /// Nothing resident covers this page, at any zoom.
    ///
    /// The all-ones slot with every other field zero. A shader that samples it
    /// anyway reads slot 4095, which is why a plan never hands out that slot.
    pub const ABSENT: Self = Self(SLOT_MASK);

    /// An entry naming a slot, a zoom and an offset.
    ///
    /// Out-of-range parts are masked to their fields rather than refused,
    /// because [`TileConfig::validate`](crate::TileConfig::validate) has
    /// already rejected every configuration that could produce one and a
    /// fallible constructor here would be an error path nothing can reach.
    #[must_use]
    pub const fn new(slot: TileSlot, level: u8, offset: [u16; 2]) -> Self {
        let slot = slot.0 as u32 & SLOT_MASK;
        let level = level as u32 & LEVEL_MASK;
        let u = offset[0] as u32 & OFFSET_MASK;
        let v = offset[1] as u32 & OFFSET_MASK;
        Self(slot | (level << LEVEL_SHIFT) | (u << OFFSET_U_SHIFT) | (v << OFFSET_V_SHIFT))
    }

    /// The slot, or `None` for [`ABSENT`](Self::ABSENT).
    #[must_use]
    pub const fn slot(self) -> Option<TileSlot> {
        let slot = self.0 & SLOT_MASK;
        if slot == SLOT_MASK {
            None
        } else {
            Some(TileSlot(slot as u16))
        }
    }

    /// The power-of-two zoom of the tile in that slot.
    #[must_use]
    pub const fn level(self) -> u8 {
        ((self.0 >> LEVEL_SHIFT) & LEVEL_MASK) as u8
    }

    /// Where the page sits inside that tile, in pages.
    #[must_use]
    pub const fn offset(self) -> [u16; 2] {
        [
            ((self.0 >> OFFSET_U_SHIFT) & OFFSET_MASK) as u16,
            ((self.0 >> OFFSET_V_SHIFT) & OFFSET_MASK) as u16,
        ]
    }

    /// Whether anything is resident for this page.
    #[must_use]
    pub const fn is_present(self) -> bool {
        self.slot().is_some()
    }

    /// The word as the device sees it.
    ///
    /// Signed because the design's table is a table of `i32`, and because a
    /// storage buffer of `i32` is the one integer type every shading language
    /// in this workspace's reach agrees on. Nothing is arithmetic on it; the
    /// shader masks.
    #[must_use]
    pub const fn bits(self) -> i32 {
        self.0.cast_signed()
    }

    /// An entry from the word the device sees.
    #[must_use]
    pub const fn from_bits(bits: i32) -> Self {
        Self(bits.cast_unsigned())
    }
}
