//! Carrying a plan out: the four steps, in the order they have to happen in.
//!
//! The seam against `cache.rs` beside it is *when*. Everything there happens
//! once, when the cache is built; everything here happens once a frame, and the
//! order it happens in is the whole of what this file is about.

use corvid_image::{Eviction, Image, TilePlan, TileSlot, TileTable, Upload};

use crate::bindings::bind_group;
use crate::cache::TileCache;
use crate::error::CacheError;
use crate::format::{device_bytes_per_texel, stage};

/// What one row of a texture upload has to be a multiple of.
const ROW_ALIGNMENT: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

impl TileCache {
    /// Give up the slots a plan's evictions name.
    ///
    /// Nothing is written and nothing is cleared: an evicted slot is one that
    /// may be overwritten, and overwriting is what the upload after it does. A
    /// slot holding something other than the tile the plan expected is left
    /// alone and reported, because that is a cache and a planner that have
    /// stopped agreeing and clearing it would hide which.
    pub fn evict(&mut self, evictions: &[Eviction]) {
        for eviction in evictions {
            let Some(slot) = self.held.get_mut(eviction.slot.index()) else {
                continue;
            };
            if *slot == Some(eviction.key) {
                *slot = None;
            } else {
                tracing::warn!(
                    name: "corvid_image_render.stale_eviction",
                    slot = eviction.slot.0,
                    expected = %eviction.key,
                    "the plan gave up a tile this cache does not hold there",
                );
            }
        }
    }

    /// Write one tile into the slot the plan chose for it.
    ///
    /// Level zero only. The rest of the chain is
    /// [`generate_mips`](Self::generate_mips)'s, and it is on the device.
    ///
    /// The write goes through the queue, which orders it before the next
    /// submit -- so the encoder `generate_mips` records into may be submitted
    /// afterwards and still see these texels.
    ///
    /// # A tile smaller than a slot
    ///
    /// A tile at the right or bottom edge of a source has fewer texels than a
    /// slot, and the rest of the slot is left as whatever was in it. Nothing
    /// samples that: a uv is clamped to the source's own extent, so no page
    /// resolves past the tile's last texel. Its *mip chain* does mix it in,
    /// because the reduction runs over the whole slot -- so a caller that
    /// intends to read an edge tile at a coarse mip should pad the tile to a
    /// whole slot before handing it over, by repeating the edge texel the way a
    /// clamped sampler would. That is a decision about the picture, which is
    /// why it is not made here.
    ///
    /// # Errors
    ///
    /// [`CacheError::NoSuchSlot`] for a slot this cache does not have, which is
    /// a plan made against a larger budget. [`CacheError::SlotOccupied`] when
    /// the plan's evictions were not performed first.
    /// [`CacheError::Format`] for a tile of a format other than the cache's,
    /// and [`CacheError::TileTooLarge`] for one bigger than a slot -- a tile at
    /// the right or bottom edge of a source is *smaller* and that is normal.
    pub fn upload(
        &mut self,
        queue: &wgpu::Queue,
        upload: &Upload,
        tile: &Image,
    ) -> Result<(), CacheError> {
        if tile.format() != self.format {
            return Err(CacheError::Format {
                wanted: self.format,
                given: tile.format(),
            });
        }
        let extent = tile.extent();
        if extent.width > self.config.tile_size || extent.height > self.config.tile_size {
            return Err(CacheError::TileTooLarge {
                extent,
                tile: self.config.tile_size,
            });
        }
        let (layer, origin) =
            self.atlas
                .locate(upload.slot)
                .ok_or_else(|| CacheError::NoSuchSlot {
                    slot: upload.slot,
                    capacity: self.atlas.slots(),
                })?;
        match self.held.get(upload.slot.index()).copied().flatten() {
            Some(held) if held != upload.key => {
                return Err(CacheError::SlotOccupied {
                    slot: upload.slot,
                    held,
                });
            }
            _ => {}
        }

        let texel = device_bytes_per_texel(self.format);
        let stride = (extent.width * texel).div_ceil(ROW_ALIGNMENT) * ROW_ALIGNMENT;
        self.scratch.clear();
        self.scratch
            .resize(stride as usize * extent.height as usize, 0);
        stage(
            &mut self.scratch,
            tile.texels(),
            self.format,
            extent.width as usize,
            extent.height as usize,
            stride as usize,
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &self.scratch,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(extent.height),
            },
            wgpu::Extent3d {
                width: extent.width,
                height: extent.height,
                depth_or_array_layers: 1,
            },
        );
        if let Some(slot) = self.held.get_mut(upload.slot.index()) {
            *slot = Some(upload.key);
        }
        self.pending.push(upload.slot);
        Ok(())
    }

    /// Record the mip chain of every tile uploaded since the last call.
    ///
    /// Nothing is submitted here: the passes go into the caller's encoder, so a
    /// frame that streams and draws is one submit rather than two.
    pub fn generate_mips(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.pending.is_empty() {
            return;
        }
        self.pending.sort_unstable();
        self.pending.dedup();
        self.mips.record(encoder, &self.atlas, &self.pending);
        self.pending.clear();
    }

    /// Publish the table a plan produced.
    ///
    /// Call it after the uploads, never before: a table describes the residency
    /// a plan *produces*, so a frame drawn from it before the tiles land samples
    /// slots whose contents have not arrived.
    ///
    /// # Errors
    ///
    /// [`CacheError::TableTileSize`] for a table built under a different
    /// [`TileConfig`](corvid_image::TileConfig) than the cache, which would put
    /// every sample somewhere else. [`CacheError::TooManySources`] and
    /// [`CacheError::TableTooWide`] for a table past what the parameter block
    /// or a texture on this device holds.
    pub fn write_table(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        table: &TileTable,
    ) -> Result<(), CacheError> {
        if table.tile_size() != self.config.tile_size {
            return Err(CacheError::TableTileSize {
                wanted: self.config.tile_size,
                given: table.tile_size(),
            });
        }
        if self
            .table
            .write(device, queue, table, &self.atlas, &self.label)?
        {
            self.binding = bind_group(
                device,
                &self.layout,
                &self.table,
                &self.view,
                &self.sampler,
                &self.label,
            );
        }
        Ok(())
    }

    /// Forget every tile, as after a device loss or a
    /// [`TilePlanner::reset`](corvid_image::TilePlanner::reset).
    ///
    /// The texture is kept and its contents are left alone; what goes is the
    /// record of what is in it, so the next plan's uploads fill it from nothing.
    pub fn reset(&mut self) {
        self.held.iter_mut().for_each(|slot| *slot = None);
        self.pending.clear();
    }

    /// Whether this plan can be carried out by this cache at all.
    ///
    /// Every upload names a slot, and a plan made against a larger budget names
    /// slots that do not exist. Checking once is cheaper than finding out on
    /// whichever upload happens to be first.
    ///
    /// # Errors
    ///
    /// [`CacheError::NoSuchSlot`] naming the first slot past the end.
    pub fn admits(&self, plan: &TilePlan) -> Result<(), CacheError> {
        let past = |slot: TileSlot| u32::from(slot.0) >= self.atlas.slots();
        let named = plan
            .uploads()
            .iter()
            .map(|upload| upload.slot)
            .chain(plan.evictions().iter().map(|eviction| eviction.slot));
        match named.filter(|slot| past(*slot)).min() {
            Some(slot) => Err(CacheError::NoSuchSlot {
                slot,
                capacity: self.atlas.slots(),
            }),
            None => Ok(()),
        }
    }
}
