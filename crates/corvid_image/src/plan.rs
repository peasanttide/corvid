//! The value a plan is: what to upload, what to give up, and what results.

use alloc::vec::Vec;

use crate::{Priority, Residency, TileKey, TileSlot, TileTable};

/// A tile to put on the device, and where to put it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Upload {
    /// Which tile.
    pub key: TileKey,
    /// The slot to write it to, already free by the time the plan's evictions
    /// have been honoured.
    pub slot: TileSlot,
    /// What it is worth, which is the order the list is in.
    pub priority: Priority,
}

/// A tile to stop keeping, and why it was the one to go.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Eviction {
    /// Which tile.
    pub key: TileKey,
    /// The slot it frees.
    pub slot: TileSlot,
    /// What it was worth this frame: `None` for a tile nothing on screen wants
    /// any more, and `Some` for one that is still wanted and still lost, which
    /// only happens when the budget cannot hold the working set.
    pub priority: Option<Priority>,
}

/// What to do to the device, and what the result will look like.
///
/// A plan is a value, not an action. Nothing here has touched a GPU: the device
/// half of this -- `corvid_image_render` -- performs the
/// [`evictions`](Self::evictions), then the [`uploads`](Self::uploads), then
/// hands the [`table`](Self::table) over, and only then calls
/// [`TilePlanner::commit`](crate::TilePlanner::commit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TilePlan {
    table: TileTable,
    uploads: Vec<Upload>,
    evictions: Vec<Eviction>,
    residency: Residency,
    capacity: u32,
    wanted: usize,
}

impl TilePlan {
    /// The lookup table for the residency this plan produces.
    #[must_use]
    pub const fn table(&self) -> &TileTable {
        &self.table
    }

    /// The tiles to fetch and write, most valuable first.
    #[must_use]
    pub fn uploads(&self) -> &[Upload] {
        &self.uploads
    }

    /// The tiles to give up, least valuable first.
    #[must_use]
    pub fn evictions(&self) -> &[Eviction] {
        &self.evictions
    }

    /// What is resident once this plan has been carried out.
    #[must_use]
    pub const fn residency(&self) -> &Residency {
        &self.residency
    }

    /// How many tiles the budget and the configuration between them allow.
    #[must_use]
    pub const fn capacity(&self) -> u32 {
        self.capacity
    }

    /// How many tiles the views between them asked for.
    #[must_use]
    pub const fn wanted(&self) -> usize {
        self.wanted
    }

    /// Whether the views asked for more than the budget can hold.
    ///
    /// A degraded plan is not a failed one. It is the frame drawn from coarser
    /// mips than were asked for, which is the behaviour the whole priority
    /// order exists to produce: something blurry now beats something sharp two
    /// frames from now.
    #[must_use]
    pub const fn is_degraded(&self) -> bool {
        self.wanted > self.capacity as usize
    }
}
impl TilePlan {
    /// A plan from the parts `TilePlanner::plan` computed.
    ///
    /// Crate-private, because a plan whose table and residency disagree is a
    /// frame that samples slots holding something else, and only the planner is
    /// in a position to know they agree.
    pub(crate) const fn new(
        table: TileTable,
        uploads: Vec<Upload>,
        evictions: Vec<Eviction>,
        residency: Residency,
        capacity: u32,
        wanted: usize,
    ) -> Self {
        Self {
            table,
            uploads,
            evictions,
            residency,
            capacity,
            wanted,
        }
    }
}
