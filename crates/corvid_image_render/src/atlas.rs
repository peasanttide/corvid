//! Where a slot is inside the tile array texture.
//!
//! The seam against the rest of the crate is that nothing here names a device.
//! It reads [`wgpu::Limits`], which is a struct of numbers, and answers a
//! layout -- so the arithmetic a fragment shader does to find a tile can be
//! checked on a machine with no adapter in it, which is the only way a shader's
//! addressing is ever actually checked.

use corvid_image::{PixelFormat, TileSlot};

use crate::format::device_bytes_per_texel;

/// How the slots of a tile cache are laid out in one array texture.
///
/// One tile per array layer is the obvious layout and it does not work: a
/// device's [`max_texture_array_layers`](wgpu::Limits::max_texture_array_layers)
/// is 256 on the limits this workspace opens a device with, and the design's
/// minimum specification asks for 2048 tiles. So a layer holds a *grid* of
/// tiles and there are a few layers, which is what puts 2048 slots on a device
/// with room for 256 layers.
///
/// Both grid dimensions are powers of two, which is not cosmetic: it is what
/// lets the fragment shader turn a slot into a layer and an origin with two
/// shifts and two masks rather than two integer divisions per sample.
///
/// ```
/// use corvid_image::{TILE_SIZE, TileSlot};
/// use corvid_image_render::Atlas;
///
/// // The minimum specification on a device with 16384-texel textures: 2048
/// // tiles as a 64 by 32 grid, which is one layer.
/// let mut limits = wgpu::Limits::downlevel_defaults();
/// limits.max_texture_dimension_2d = 16384;
/// let atlas = Atlas::plan(TILE_SIZE, 2048, &limits).ok_or("a tile fits a texture")?;
///
/// assert_eq!(atlas.slots(), 2048);
/// assert_eq!(atlas.layers(), 1);
/// assert_eq!(atlas.layer_extent(), (16384, 8192));
///
/// // Slot 65 is the first cell of the grid's second row, plus one.
/// assert_eq!(atlas.locate(TileSlot(65)), Some((0, [TILE_SIZE, TILE_SIZE])));
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Atlas {
    tile_size: u32,
    across_shift: u32,
    down_shift: u32,
    layers: u32,
    slots: u32,
}

impl Atlas {
    /// The layout for `wanted` tiles of `tile_size` texels on a device with
    /// these limits, or `None` if one tile is larger than a texture or
    /// `tile_size` is not a power of two.
    ///
    /// The grid is as square as it can be, because a square grid is the one
    /// that wastes the fewest cells of the last layer, and is then clamped to
    /// what a texture on this device reaches. `wanted` is a request rather than
    /// a promise: [`slots`](Self::slots) is what came back, and it is smaller
    /// when the device's layer limit bites.
    #[must_use]
    pub fn plan(tile_size: u32, wanted: u32, limits: &wgpu::Limits) -> Option<Self> {
        if tile_size == 0 || !tile_size.is_power_of_two() {
            return None;
        }
        let cells = limits.max_texture_dimension_2d / tile_size;
        if cells == 0 {
            return None;
        }
        // `cells` is at least one, so its log is defined: it is how many tiles a
        // layer may be across or down before the texture is too wide.
        let cap = cells.ilog2();
        let wanted = wanted.max(1);
        let across_shift = ceil_log2(ceil_sqrt(wanted)).min(cap);
        let rows = wanted.div_ceil(1 << across_shift);
        // Never taller than it is wide: a grid growing downwards past its own
        // width would be a long thin layer with no more cells in it.
        let down_shift = ceil_log2(rows).min(across_shift);
        let per_layer = 1u32 << (across_shift + down_shift);
        let layers = wanted
            .div_ceil(per_layer)
            .clamp(1, limits.max_texture_array_layers.max(1));
        Some(Self {
            tile_size,
            across_shift,
            down_shift,
            layers,
            slots: wanted.min(layers.saturating_mul(per_layer)),
        })
    }

    /// This layout with only as many layers as `bytes` pays for.
    ///
    /// The grid stays as it is and the layers go, which is what keeps a cache
    /// inside a budget without changing the arithmetic the shader was built
    /// with. One layer is the floor: a cache with no texture in it is not a
    /// smaller cache, and the budget floor in `budget.rs` is what keeps that
    /// case out of reach.
    #[must_use]
    pub fn fitted(self, format: PixelFormat, bytes: u64) -> Self {
        let each = self.layer_bytes(format);
        if each == 0 {
            return self;
        }
        let afford = u32::try_from(bytes / each).unwrap_or(u32::MAX);
        let layers = afford.clamp(1, self.layers.max(1));
        let room = layers.saturating_mul(self.per_layer());
        Self {
            layers,
            slots: self.slots.min(room),
            ..self
        }
    }

    /// The tile side this layout was planned for, in texels.
    #[must_use]
    pub const fn tile_size(&self) -> u32 {
        self.tile_size
    }

    /// How many tiles this layout holds, which is what a plan's budget is
    /// capped at.
    #[must_use]
    pub const fn slots(&self) -> u32 {
        self.slots
    }

    /// How many array layers the texture has.
    #[must_use]
    pub const fn layers(&self) -> u32 {
        self.layers
    }

    /// How many tiles one layer is across.
    #[must_use]
    pub const fn tiles_across(&self) -> u32 {
        1 << self.across_shift
    }

    /// How many tiles one layer is down.
    #[must_use]
    pub const fn tiles_down(&self) -> u32 {
        1 << self.down_shift
    }

    /// How many tiles one layer holds, which is a power of two.
    #[must_use]
    pub const fn per_layer(&self) -> u32 {
        1 << (self.across_shift + self.down_shift)
    }

    /// The two shifts the shader is handed: the log of
    /// [`tiles_across`](Self::tiles_across), and the log of
    /// [`per_layer`](Self::per_layer).
    ///
    /// Public because the parameter block carries them, and a caller reading
    /// that block should be able to say what is in it.
    #[must_use]
    pub const fn shifts(&self) -> (u32, u32) {
        (self.across_shift, self.across_shift + self.down_shift)
    }

    /// How big one layer is, in texels.
    #[must_use]
    pub const fn layer_extent(&self) -> (u32, u32) {
        (
            self.tiles_across() * self.tile_size,
            self.tiles_down() * self.tile_size,
        )
    }

    /// How many mip levels a tile has, counting level zero.
    ///
    /// The chain stops where a *tile* is one texel rather than where a layer is,
    /// because a level below that would average two tiles together and the whole
    /// point of an aligned grid is that no level of it ever does.
    #[must_use]
    pub const fn mip_levels(&self) -> u32 {
        self.tile_size.ilog2() + 1
    }

    /// Which layer a slot is in, and where its tile starts there in texels.
    ///
    /// `None` for a slot this layout does not have. This is the Rust half of
    /// what the WGSL does with two shifts and two masks, and `tests/geometry.rs`
    /// is what holds the two halves together.
    #[must_use]
    #[allow(
        clippy::cast_lossless,
        reason = "a const fn cannot call a trait method, so `From` is out of reach; the cast is u16 to u32 and widening"
    )]
    pub const fn locate(&self, slot: TileSlot) -> Option<(u32, [u32; 2])> {
        let slot = slot.0 as u32;
        if slot >= self.slots {
            return None;
        }
        let per_layer_shift = self.across_shift + self.down_shift;
        let cell = slot & ((1 << per_layer_shift) - 1);
        Some((
            slot >> per_layer_shift,
            [
                (cell & ((1 << self.across_shift) - 1)) * self.tile_size,
                (cell >> self.across_shift) * self.tile_size,
            ],
        ))
    }

    /// How many bytes one layer weighs, mip chain and all.
    #[must_use]
    #[allow(
        clippy::cast_lossless,
        reason = "a const fn cannot call a trait method, so `From` is out of reach; every cast here is u32 to u64 and widening"
    )]
    pub const fn layer_bytes(&self, format: PixelFormat) -> u64 {
        let (width, height) = self.layer_extent();
        let texel = device_bytes_per_texel(format) as u64;
        let mut total = 0u64;
        let mut level = 0;
        while level < self.mip_levels() {
            total += (width >> level) as u64 * (height >> level) as u64 * texel;
            level += 1;
        }
        total
    }

    /// How many bytes the whole tile array weighs, mip chain and all.
    ///
    /// This is the number a budget is checked against, and it counts the cells
    /// of a partly filled last layer: a texture is allocated whole whether or
    /// not every cell of it is ever written.
    #[must_use]
    #[allow(
        clippy::cast_lossless,
        reason = "a const fn cannot call a trait method, so `From` is out of reach; the cast is u32 to u64 and widening"
    )]
    pub const fn bytes(&self, format: PixelFormat) -> u64 {
        self.layer_bytes(format) * self.layers as u64
    }
}

/// The smallest `n` with `2^n >= value`.
const fn ceil_log2(value: u32) -> u32 {
    if value <= 1 {
        0
    } else {
        (value - 1).ilog2() + 1
    }
}

/// The smallest `n` with `n * n >= value`.
const fn ceil_sqrt(value: u32) -> u32 {
    let root = value.isqrt();
    if root * root < value { root + 1 } else { root }
}
