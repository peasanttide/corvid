//! What a tile cache *is*: the three device resources it owns and the numbers
//! it was built from.
//!
//! The seam against `apply.rs` beside it is *when*. Everything here happens
//! once, when the cache is built; everything there happens once a frame, when a
//! plan is carried out. `bindings.rs` is the descriptors either of them fills.

use alloc::vec::Vec;

use corvid_image::{PixelFormat, TileConfig, TileKey, TileSlot, VramBudget};

use crate::atlas::Atlas;
use crate::bindings::{bind_group, bind_group_layout, tile_texture};
use crate::budget::resident_tiles;
use crate::error::CacheError;
use crate::format::texture_format;
use crate::mips::Mips;
use crate::table::Table;

/// A `corvid_image` tile plan, made real on a device.
///
/// It owns three things: the array texture the tiles live in, the texture the
/// lookup table lives in, and the parameter block that tells a shader how to
/// read either. What it does not own is a planner -- [`TilePlanner`] stays on
/// the other side of the fence, decides everything, and hands this a value.
///
/// # The four steps, in this order
///
/// [`evict`](Self::evict) frees the slots the plan gave up, then
/// [`upload`](Self::upload) writes the tiles it asked for, then
/// [`generate_mips`](Self::generate_mips) records the reduction of what was
/// written, then [`write_table`](Self::write_table) publishes the table that
/// describes the result. Only then does the caller call
/// [`TilePlanner::commit`].
///
/// The order is not a style. The planner hands a freed slot straight back out
/// as somewhere to upload to, so uploading before evicting overwrites a tile
/// the table still points at -- and [`upload`](Self::upload) answers
/// [`CacheError::SlotOccupied`] rather than doing it. Writing the table before
/// the tiles have landed is the same mistake pointing the other way: a frame
/// that samples a slot whose contents are still in flight.
///
/// ```
/// use corvid_image::{PixelFormat, SourceView, TileConfig, TilePlanner, VramBudget, extent};
/// use corvid_image_render::TileCache;
///
/// # fn stream(
/// #     device: &wgpu::Device,
/// #     queue: &wgpu::Queue,
/// #     encoder: &mut wgpu::CommandEncoder,
/// #     fetch: impl Fn(corvid_image::TileKey) -> Option<corvid_image::Image>,
/// # ) -> Result<(), Box<dyn core::error::Error>> {
/// let config = TileConfig::MIN_SPEC;
/// let budget = VramBudget::MIN_SPEC.share(25);
/// let mut cache = TileCache::new(device, config, budget, PixelFormat::SRGB8, "map")?;
///
/// let mut planner = TilePlanner::new(config)?;
/// let plate = planner.register(extent(16384, 16384), PixelFormat::SRGB8)?;
/// let plan = planner.plan(&[SourceView::full(plate)], budget);
///
/// cache.evict(plan.evictions());
/// for upload in plan.uploads() {
///     if let Some(tile) = fetch(upload.key) {
///         cache.upload(queue, upload, &tile)?;
///     }
/// }
/// cache.generate_mips(encoder);
/// cache.write_table(device, queue, plan.table())?;
/// planner.commit(&plan);
/// # Ok(())
/// # }
/// ```
///
/// [`TilePlanner`]: corvid_image::TilePlanner
/// [`TilePlanner::commit`]: corvid_image::TilePlanner::commit
#[derive(Debug)]
pub struct TileCache {
    pub(crate) config: TileConfig,
    pub(crate) format: PixelFormat,
    pub(crate) texture_format: wgpu::TextureFormat,
    pub(crate) budget: VramBudget,
    pub(crate) atlas: Atlas,
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) sampler: wgpu::Sampler,
    pub(crate) table: Table,
    pub(crate) mips: Mips,
    pub(crate) layout: wgpu::BindGroupLayout,
    pub(crate) binding: wgpu::BindGroup,
    /// What each slot holds, indexed by slot.
    pub(crate) held: Vec<Option<TileKey>>,
    /// The slots written since the last [`TileCache::generate_mips`].
    pub(crate) pending: Vec<TileSlot>,
    /// Row-padded staging for one tile, kept so a steady stream allocates once.
    pub(crate) scratch: Vec<u8>,
    pub(crate) label: alloc::string::String,
}

impl TileCache {
    /// Build a cache of `format` tiles inside `budget` on this device.
    ///
    /// The slot count is the smaller of what the budget pays for -- mip chain
    /// and the widening of a three-channel texel included, which
    /// [`resident_tiles`](crate::resident_tiles) is -- and what the device's
    /// texture limits allow. [`slots`](Self::slots) is the answer, and it is
    /// what a [`VramBudget`] handed to a planner should be capped against.
    ///
    /// # Errors
    ///
    /// [`CacheError::Config`] for a configuration the packed table entry has no
    /// bits for, [`CacheError::NoTextureFormat`] for a pixel format no
    /// eight-bit texture holds, and [`CacheError::TileTooBig`] for a tile side
    /// past this device's largest texture.
    pub fn new(
        device: &wgpu::Device,
        config: TileConfig,
        budget: VramBudget,
        format: PixelFormat,
        label: &str,
    ) -> Result<Self, CacheError> {
        config.validate()?;
        let texture_format = texture_format(format).ok_or(CacheError::NoTextureFormat(format))?;
        let limits = device.limits();
        let wanted = resident_tiles(budget, &config, format);
        let atlas = Atlas::plan(config.tile_size, wanted, &limits)
            .ok_or(CacheError::TileTooBig {
                tile: config.tile_size,
                max: limits.max_texture_dimension_2d,
            })?
            .fitted(format, budget.bytes);

        let texture = tile_texture(device, label, &atlas, texture_format);
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(label),
            // Clamped, though `corvid_tile_point` has already kept the sample
            // inside its own tile: the address mode is what catches the caller
            // who binds this texture themselves.
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let table = Table::new(device, label);
        let mips = Mips::new(device, &texture, &atlas, texture_format, label);
        let layout = bind_group_layout(device, label);
        let binding = bind_group(device, &layout, &table, &view, &sampler, label);

        tracing::debug!(
            name: "corvid_image_render.cache",
            slots = atlas.slots(),
            layers = atlas.layers(),
            across = atlas.tiles_across(),
            down = atlas.tiles_down(),
            bytes = atlas.bytes(format),
            budget = budget.bytes,
            "built a tile cache",
        );

        Ok(Self {
            config,
            format,
            texture_format,
            budget,
            atlas,
            texture,
            view,
            sampler,
            table,
            mips,
            layout,
            binding,
            held: alloc::vec![None; atlas.slots() as usize],
            pending: Vec::new(),
            scratch: Vec::new(),
            label: label.into(),
        })
    }

    /// The numbers this cache was built with.
    #[must_use]
    pub const fn config(&self) -> &TileConfig {
        &self.config
    }

    /// What a texel of every tile in it is.
    #[must_use]
    pub const fn format(&self) -> PixelFormat {
        self.format
    }

    /// What that format is on the device, which is not always the same shape:
    /// see [`texture_format`](crate::texture_format).
    #[must_use]
    pub const fn texture_format(&self) -> wgpu::TextureFormat {
        self.texture_format
    }

    /// The budget it was asked to fit in.
    #[must_use]
    pub const fn budget(&self) -> VramBudget {
        self.budget
    }

    /// How the slots are laid out in the array texture.
    #[must_use]
    pub const fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// How many tiles it holds, which is what to cap a plan's budget at.
    #[must_use]
    pub const fn slots(&self) -> u32 {
        self.atlas.slots()
    }

    /// How many bytes the tile array occupies, mip chain and all.
    ///
    /// The lookup table is not in this: it is one word a page and it follows
    /// the sources rather than the budget, which for the largest configuration
    /// there is comes to a few megabytes against the cache's hundreds.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.atlas.bytes(self.format)
    }

    /// How many slots hold a tile.
    #[must_use]
    pub fn occupied(&self) -> usize {
        self.held.iter().filter(|slot| slot.is_some()).count()
    }

    /// What this slot holds, if anything.
    #[must_use]
    pub fn holds(&self, slot: TileSlot) -> Option<TileKey> {
        self.held.get(slot.index()).copied().flatten()
    }

    /// The bind group layout a pipeline names for the group
    /// [`WGSL`](crate::WGSL) declares.
    #[must_use]
    pub const fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// The bind group to set at that group index.
    ///
    /// Rebuilt whenever [`write_table`](Self::write_table) has to make a bigger
    /// table texture, so read it each frame rather than keeping a copy.
    #[must_use]
    pub const fn bind_group(&self) -> &wgpu::BindGroup {
        &self.binding
    }
}
