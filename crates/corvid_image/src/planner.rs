//! The machine that turns what the viewer can see into a plan.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

use crate::{
    Eviction, Extent, PixelFormat, Priority, Residency, Source, SourceId, SourceView, Sources,
    Tier, TileConfig, TileError, TileKey, TilePlan, TileSlot, TileTable, Upload, UvRect,
    VramBudget,
};

/// How many times the tile budget the want set is allowed to grow to before
/// enumeration stops.
///
/// The enumeration runs coarse level first, so cutting it short can only ever
/// drop the finest tiles -- which are also the ones the ranking would have cut
/// anyway. Without the bound, a source at level zero across a whole screen is
/// a quarter of a million map inserts every frame for a plan that will keep two
/// thousand of them.
const ENUMERATION_SLACK: usize = 4;

/// The CPU half of the streamer: it knows what exists, what is resident, and
/// what the eye is on, and it turns those into a plan.
///
/// It never reads a pixel and never touches a device. Registering a source
/// takes its size and its format, not its bytes, because the whole point of
/// tiling a 131072-texel plate is that its bytes are never all in one place.
///
/// ```
/// use corvid_image::{
///     PixelFormat, SourceView, TileConfig, TilePlanner, VramBudget, extent,
/// };
///
/// let mut planner = TilePlanner::new(TileConfig::MIN_SPEC)?;
/// let plate = planner.register(extent(16384, 16384), PixelFormat::SRGB8)?;
///
/// let plan = planner.plan(&[SourceView::full(plate)], VramBudget::MIN_SPEC);
/// assert!(!plan.uploads().is_empty());
/// assert!(plan.evictions().is_empty());
///
/// // The plan is a value; nothing has happened yet.
/// assert!(planner.residency().is_empty());
/// planner.commit(&plan);
/// assert_eq!(planner.residency().len(), plan.uploads().len());
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TilePlanner {
    config: TileConfig,
    sources: Sources,
    residency: Residency,
}

impl TilePlanner {
    /// A planner with nothing registered and nothing resident.
    ///
    /// # Errors
    ///
    /// [`ConfigError`](crate::ConfigError) if the configuration is a shape the
    /// packed table entry has no bits for. Checking it once here is what lets
    /// every later step be infallible arithmetic.
    pub fn new(config: TileConfig) -> Result<Self, crate::ConfigError> {
        config.validate()?;
        Ok(Self {
            config,
            sources: Sources::new(),
            residency: Residency::new(),
        })
    }

    /// The numbers this planner was built with.
    #[must_use]
    pub const fn config(&self) -> &TileConfig {
        &self.config
    }

    /// Everything registered.
    #[must_use]
    pub const fn sources(&self) -> &Sources {
        &self.sources
    }

    /// What is on the device right now.
    #[must_use]
    pub const fn residency(&self) -> &Residency {
        &self.residency
    }

    /// Register a picture, answering the id the plan will name it by.
    ///
    /// # Errors
    ///
    /// [`TileError::TooLarge`] for a picture past
    /// [`TileConfig::max_image_size`], [`TileError::Empty`] for one with no
    /// texels, and [`TileError::TooManySources`] once the configured maximum
    /// are registered.
    pub fn register(&mut self, extent: Extent, format: PixelFormat) -> Result<SourceId, TileError> {
        self.sources.push(&self.config, extent, format)
    }

    /// Plan residency for these views under this budget.
    ///
    /// A pure function of the planner's state and its two arguments. Calling it
    /// twice answers twice the same plan, and that is a property rather than a
    /// coincidence: every collection it walks is ordered, the weights are
    /// quantised before anything compares them, and the tie between two equally
    /// valuable tiles is broken by [`TileKey`] rather than by whichever was
    /// enumerated first.
    ///
    /// Two views naming one source merge -- the union of their rectangles, the
    /// finer of their levels, the greater of their weights -- so the answer
    /// does not depend on the order the views arrived in either.
    #[must_use]
    pub fn plan(&self, views: &[SourceView], budget: VramBudget) -> TilePlan {
        let capacity = budget.capacity(&self.config, self.widest_format());
        let merged = self.merge(views);
        let wants = self.enumerate(&merged, capacity);
        self.assemble(&merged, &wants, capacity)
    }

    /// Take a plan's residency as the truth, once the device has carried it
    /// out.
    pub fn commit(&mut self, plan: &TilePlan) {
        self.residency = plan.residency().clone();
    }

    /// Forget every resident tile, as after a device loss.
    pub fn reset(&mut self) {
        self.residency.clear();
    }

    /// The widest texel of any registered source, which is what a tile is
    /// costed at.
    ///
    /// One number for the whole cache rather than one per source, because a
    /// slot has to be able to hold any tile that lands in it, and costing the
    /// budget at the widest format is the difference between a plan that fits
    /// and a plan that fits on average.
    fn widest_format(&self) -> PixelFormat {
        self.sources
            .iter()
            .map(|(_, source)| source.format())
            .max_by_key(|format| format.bytes_per_texel())
            .unwrap_or(PixelFormat::SRGBA8)
    }

    /// Fold the views down to at most one per source.
    fn merge(&self, views: &[SourceView]) -> BTreeMap<SourceId, Merged> {
        let mut merged: BTreeMap<SourceId, Merged> = BTreeMap::new();
        for view in views {
            let Some(source) = self.sources.get(view.source) else {
                continue;
            };
            let rect = view.visible.clipped();
            if rect.is_empty() {
                continue;
            }
            let fresh = Merged {
                rect,
                level: view.level(source),
                rank: view.rank(),
            };
            merged
                .entry(view.source)
                .and_modify(|held| held.absorb(&fresh))
                .or_insert(fresh);
        }
        merged
    }

    /// Every tile the views want, coarsest level first, bounded.
    fn enumerate(
        &self,
        merged: &BTreeMap<SourceId, Merged>,
        capacity: u32,
    ) -> BTreeMap<TileKey, Priority> {
        let bound = (capacity as usize).saturating_mul(ENUMERATION_SLACK);
        let mut wants: BTreeMap<TileKey, Priority> = BTreeMap::new();
        for level in (0..=TileConfig::MAX_LEVEL).rev() {
            if wants.len() >= bound {
                break;
            }
            for (id, want) in merged {
                let Some(source) = self.sources.get(*id) else {
                    continue;
                };
                if level > source.top_level() || level < want.level {
                    continue;
                }
                let tier = if level == source.top_level() {
                    Tier::Root
                } else {
                    Tier::Detail
                };
                let priority = Priority::new(tier, want.rank, level);
                for key in self.tiles_under(*id, source, want.rect, level) {
                    wants.insert(key, priority);
                }
            }
        }
        wants
    }

    /// The tiles of `source` at `level` that `rect` touches.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a tile count is at most 8192, which an f32 holds exactly; the product is then clamped to that count, and a float cast in Rust saturates rather than wrapping, so a NaN lands on tile zero"
    )]
    fn tiles_under(
        &self,
        id: SourceId,
        source: &Source,
        rect: UvRect,
        level: u8,
    ) -> impl Iterator<Item = TileKey> {
        let [across, down] = source.tiles_at(&self.config, u32::from(level));
        // The rectangle is half open, so an edge landing exactly on a tile
        // boundary belongs to the tile before it. Getting this wrong is a whole
        // extra row and column of tiles requested for a view that does not
        // touch them, which at level zero on a large plate is most of the
        // budget spent on nothing.
        let first = |value: f32, count: u32| ((value * count as f32) as u32).min(count - 1);
        let last = |value: f32, count: u32| {
            let edge = value * count as f32;
            let floor = edge as u32;
            let index = if edge > floor as f32 {
                floor
            } else {
                floor.saturating_sub(1)
            };
            index.min(count - 1)
        };
        let x0 = first(rect.min[0], across);
        let x1 = last(rect.max[0], across).max(x0);
        let y0 = first(rect.min[1], down);
        let y1 = last(rect.max[1], down).max(y0);
        (y0..=y1)
            .flat_map(move |y| (x0..=x1).map(move |x| TileKey::new(id, level, x as u16, y as u16)))
    }

    /// Rank the wants against what is already there, cut to the budget, and
    /// write down the difference.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a slot index runs to capacity, which validate caps below 4096"
    )]
    fn assemble(
        &self,
        merged: &BTreeMap<SourceId, Merged>,
        wants: &BTreeMap<TileKey, Priority>,
        capacity: u32,
    ) -> TilePlan {
        let held = |key: TileKey| {
            self.residency
                .slot(key)
                .filter(|slot| (slot.index() as u32) < capacity)
        };

        // Wanted tiles first, by value; then whatever else is resident, which
        // costs nothing to keep while there is room and is the first thing to
        // go when there is not. A tile already on the device wins a tie against
        // an equally valuable one that is not, because the alternative is
        // uploading two tiles to end up where we started.
        let mut ranked: Vec<(Option<Priority>, bool, TileKey)> = wants
            .iter()
            .map(|(key, priority)| (Some(*priority), held(*key).is_some(), *key))
            .collect();
        ranked.extend(
            self.residency
                .iter()
                .filter(|(key, _)| !wants.contains_key(key) && held(*key).is_some())
                .map(|(key, _)| (None, true, key)),
        );
        ranked.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
        ranked.truncate(capacity as usize);

        let keep: BTreeSet<TileKey> = ranked.iter().map(|(_, _, key)| *key).collect();

        let mut evictions: Vec<Eviction> = self
            .residency
            .iter()
            .filter(|(key, _)| !keep.contains(key))
            .map(|(key, slot)| Eviction {
                key,
                slot,
                priority: wants.get(&key).copied(),
            })
            .collect();
        evictions.sort_unstable_by(|a, b| a.priority.cmp(&b.priority).then(a.key.cmp(&b.key)));

        let taken: BTreeSet<TileSlot> = keep.iter().filter_map(|key| held(*key)).collect();
        let mut free = (0..capacity)
            .map(|slot| TileSlot(slot as u16))
            .filter(|slot| !taken.contains(slot));

        let mut residency = Residency::new();
        let mut uploads = Vec::new();
        for (priority, _, key) in &ranked {
            if let Some(slot) = held(*key) {
                residency.insert(*key, slot);
            } else if let Some(slot) = free.next() {
                residency.insert(*key, slot);
                if let Some(priority) = *priority {
                    uploads.push(Upload {
                        key: *key,
                        slot,
                        priority,
                    });
                }
            }
        }

        let desired: Vec<u8> = self.desired_levels(merged);
        let table = TileTable::build(&self.config, &self.sources, &residency, &desired);
        let wanted = wants.len();
        if wanted > capacity as usize {
            tracing::debug!(
                wanted,
                capacity,
                evicted = evictions.len(),
                "tile budget is smaller than the working set; serving coarser mips"
            );
        }
        TilePlan::new(table, uploads, evictions, residency, capacity, wanted)
    }

    /// The finest level each source is being asked for, indexed by source id.
    fn desired_levels(&self, merged: &BTreeMap<SourceId, Merged>) -> Vec<u8> {
        self.sources
            .iter()
            .map(|(id, source)| {
                merged.get(&id).map_or(source.top_level(), |want| {
                    want.level.min(source.top_level())
                })
            })
            .collect()
    }
}

/// Every view of one source, folded into one.
#[derive(Clone, Copy, Debug)]
struct Merged {
    rect: UvRect,
    level: u8,
    rank: u16,
}

impl Merged {
    /// Absorb another view of the same source.
    ///
    /// The union of the rectangles, the finer level and the greater weight, all
    /// three of which are commutative -- which is what keeps a plan independent
    /// of the order the views were handed over in.
    fn absorb(&mut self, other: &Self) {
        self.rect = UvRect::new(
            [
                self.rect.min[0].min(other.rect.min[0]),
                self.rect.min[1].min(other.rect.min[1]),
            ],
            [
                self.rect.max[0].max(other.rect.max[0]),
                self.rect.max[1].max(other.rect.max[1]),
            ],
        );
        self.level = self.level.min(other.level);
        self.rank = self.rank.max(other.rank);
    }
}
