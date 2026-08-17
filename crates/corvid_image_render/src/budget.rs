//! How much of a card the tile cache is allowed to be.
//!
//! The seam here is that `wgpu` has no memory query at all. There is no
//! `Adapter::vram()` and no `heap_size` in [`wgpu::AdapterInfo`], because
//! WebGPU deliberately does not expose one -- a page that could read how much
//! memory a card has could fingerprint it. So everything below is a
//! *derivation* from the two things an adapter does say, and it is written to
//! be honest about that rather than to look precise.

use corvid_image::{MIN_SPEC_VRAM, PixelFormat, TileConfig, VramBudget};

use crate::format::device_bytes_per_texel;

/// What share of a card the tile cache takes, as a whole percentage.
///
/// A quarter. The rest of a frame -- the render targets, the depth buffer, the
/// meshes, the UI atlas, whatever the game itself allocates -- comes out of the
/// same card, and a streamer that took half of one would be the reason
/// everything else stopped fitting. A quarter of the design's minimum
/// specification is exactly the design's 2048 tiles, which is not a
/// coincidence: it is where the number came from.
pub const CACHE_SHARE: u32 = 25;

/// The budget a machine gets no matter what the adapter says, in bytes.
///
/// [`CACHE_SHARE`] of [`MIN_SPEC_VRAM`], which is the design's minimum
/// specification. A machine below it is not a machine this shrinks for; it is a
/// machine that does not meet the minimum specification, and a cache that
/// quietly halved itself there would turn "the map is blurry on that laptop"
/// into a bug nobody can find. Report the blur -- `TilePlan::is_degraded` is
/// how -- rather than hiding it in a smaller budget.
pub const FLOOR: u64 = VramBudget::MIN_SPEC.share(CACHE_SHARE).bytes;

/// How much memory this adapter is treated as having, in bytes.
///
/// **This is an estimate and there is no exact answer available.** `wgpu`
/// exposes no memory size, so what is used is
/// [`max_buffer_size`](wgpu::Limits::max_buffer_size) -- the largest single
/// allocation the driver admits to, which on every desktop driver is bounded by
/// the device-local heap and is therefore correlated with the card -- corrected
/// by what kind of device it is:
///
/// A discrete GPU is taken at its word, because its heap is its own. An
/// integrated or virtual one is halved, because the memory it reports is shared
/// with the operating system and the game's own heap is already in it. A CPU
/// adapter -- a software rasteriser, which is what a build machine has -- is
/// given [`MIN_SPEC_VRAM`] flat, because "system memory" is not a texture
/// budget and a quarter of a build machine's RAM spent on tiles is a build
/// machine that swaps.
///
/// The correction is coarse on purpose. The consequence of being wrong is a
/// cache one step blurrier or one step larger than it might have been, and a
/// number with more decimal places in it would not be any more true.
#[must_use]
pub fn vram_estimate(adapter: &wgpu::Adapter) -> u64 {
    let reported = adapter.limits().max_buffer_size;
    match adapter.get_info().device_type {
        wgpu::DeviceType::DiscreteGpu => reported,
        wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::VirtualGpu => reported / 2,
        wgpu::DeviceType::Cpu | wgpu::DeviceType::Other => MIN_SPEC_VRAM,
    }
}

/// What the tile cache may occupy on this adapter: [`CACHE_SHARE`] of
/// [`vram_estimate`], never below [`FLOOR`].
///
/// A caller that already knows better -- a game with a settings screen, a test
/// pinning a small cache -- builds a [`VramBudget`] itself and never calls this.
/// It is the answer for the caller that has an adapter and nothing else.
///
/// ```
/// use corvid_image_render::{FLOOR, vram_budget};
///
/// # fn choose(adapter: &wgpu::Adapter) {
/// let budget = vram_budget(adapter);
/// assert!(budget.bytes >= FLOOR);
/// # }
/// ```
#[must_use]
pub fn vram_budget(adapter: &wgpu::Adapter) -> VramBudget {
    let estimate = vram_estimate(adapter);
    let budget = VramBudget::new(estimate).share(CACHE_SHARE);
    let bytes = budget.bytes.max(FLOOR);
    tracing::debug!(
        name: "corvid_image_render.budget",
        estimate,
        share = CACHE_SHARE,
        bytes,
        floored = bytes > budget.bytes,
        "chose a tile cache budget",
    );
    VramBudget::new(bytes)
}

/// How many tiles of `format` a budget holds on a device, mip chain and all.
///
/// Not [`VramBudget::capacity`], and the difference is the two things a device
/// costs that a file does not. A mip chain is a third again -- the plan says
/// which mip of which tile and the device generates the rest, so the rest is on
/// the card. And a three-channel picture is four bytes a texel once it is a
/// texture, because no device has a three-byte texel. A cache sized from
/// [`VramBudget::capacity`] would be over its budget by both of those together
/// on the first frame.
///
/// The answer is capped by [`TileConfig::max_tiles`] for the reason that method
/// caps it: the slot field of a table entry is twelve bits wide, and the
/// minimum specification is the number the shader was written against.
///
/// ```
/// use corvid_image::{MAX_NUM_TILES, PixelFormat, TileConfig, VramBudget};
/// use corvid_image_render::{FLOOR, resident_tiles};
///
/// let config = TileConfig::MIN_SPEC;
/// let floor = VramBudget::new(FLOOR);
///
/// // The design's floor holds the design's tile count, in the archive's own
/// // three-channel scans, with room left for the mips.
/// assert_eq!(resident_tiles(floor, &config, PixelFormat::SRGB8), MAX_NUM_TILES);
/// ```
#[must_use]
pub fn resident_tiles(budget: VramBudget, config: &TileConfig, format: PixelFormat) -> u32 {
    let texel = u64::from(device_bytes_per_texel(format));
    let side = u64::from(config.tile_size);
    // Four thirds, rounded up: the exact sum of a nine-level chain is a shade
    // under it, so this errs towards a cache that fits.
    let each = (side * side * texel * 4).div_ceil(3);
    if each == 0 {
        return 0;
    }
    u32::try_from(budget.bytes / each)
        .unwrap_or(u32::MAX)
        .min(config.max_tiles)
}
