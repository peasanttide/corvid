//! Which tiles are on the device, and where each one sits.

use alloc::collections::BTreeMap;

use crate::{TileKey, TileSlot};

/// The tiles the device holds, keyed by which tile they are.
///
/// A [`BTreeMap`] rather than a hash map, and that is the crate's central
/// promise rather than a preference: every list a plan produces is built by
/// walking this, and a plan that reshuffled between two runs with the same
/// input would be a streamer nobody could debug and a replay nobody could
/// trust. A hash map's iteration order is not part of its contract; this one's
/// is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Residency {
    slots: BTreeMap<TileKey, TileSlot>,
}

impl Residency {
    /// Nothing resident.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: BTreeMap::new(),
        }
    }

    /// How many tiles are resident.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether none are.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Where this tile is, if it is anywhere.
    #[must_use]
    pub fn slot(&self, key: TileKey) -> Option<TileSlot> {
        self.slots.get(&key).copied()
    }

    /// Whether this exact tile is resident.
    #[must_use]
    pub fn contains(&self, key: TileKey) -> bool {
        self.slots.contains_key(&key)
    }

    /// Every resident tile, in [`TileKey`] order.
    pub fn iter(&self) -> impl Iterator<Item = (TileKey, TileSlot)> + '_ {
        self.slots.iter().map(|(key, slot)| (*key, *slot))
    }

    /// Record a tile as resident in a slot, answering what it displaced there.
    pub fn insert(&mut self, key: TileKey, slot: TileSlot) -> Option<TileSlot> {
        self.slots.insert(key, slot)
    }

    /// Forget a tile, answering the slot it freed.
    pub fn remove(&mut self, key: TileKey) -> Option<TileSlot> {
        self.slots.remove(&key)
    }

    /// Forget everything.
    pub fn clear(&mut self) {
        self.slots.clear();
    }

    /// The tile serving `key`'s texels: `key` itself if it is resident, else
    /// the nearest coarser tile covering the same ground, up to `top_level`.
    ///
    /// This is what makes an overloaded streamer degrade instead of stall. A
    /// zoom that has not arrived yet is not an error and not a hole; it is the
    /// next coarser zoom, blurrier and already there. The walk is a shift per
    /// step rather than a search, because the tile covering the same ground at
    /// level `L` is at `(x >> (L - level), y >> (L - level))` and nothing else
    /// can be.
    ///
    /// ```
    /// use corvid_image::{Residency, SourceId, TileKey, TileSlot};
    ///
    /// let mut resident = Residency::new();
    /// // Tile 37 at level one is inside tile 37 >> 3 at level four.
    /// let coarse = TileKey::new(SourceId(0), 4, 4, 0);
    /// resident.insert(coarse, TileSlot(7));
    ///
    /// // Nothing at level one covers this, so the level-four tile answers.
    /// let wanted = TileKey::new(SourceId(0), 1, 37, 3);
    /// assert_eq!(resident.nearest_at_or_coarser(wanted, 8), Some((coarse, TileSlot(7))));
    ///
    /// // And a source with nothing resident at all answers nothing.
    /// let elsewhere = TileKey::new(SourceId(1), 1, 37, 3);
    /// assert_eq!(resident.nearest_at_or_coarser(elsewhere, 8), None);
    /// ```
    #[must_use]
    pub fn nearest_at_or_coarser(
        &self,
        key: TileKey,
        top_level: u8,
    ) -> Option<(TileKey, TileSlot)> {
        for level in key.level..=top_level {
            let coarser = key.at_level(level);
            if let Some(slot) = self.slot(coarser) {
                return Some((coarser, slot));
            }
        }
        None
    }
}
