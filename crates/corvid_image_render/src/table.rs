//! The lookup table on the device: one `i32` a page, one layer a source.
//!
//! The seam against `cache.rs` is that nothing here knows what a tile is. It
//! takes the words a [`TileTable`] already computed and gets them onto a texture
//! the fragment shader can load from, which is a question about row padding and
//! about when a texture has to be thrown away and made again.

use alloc::vec::Vec;

use corvid_image::TileTable;

use crate::atlas::Atlas;
use crate::error::CacheError;
use crate::params::Params;

/// What one row of a texture upload has to be a multiple of.
///
/// `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`, spelled here because the staging
/// buffer below is sized from it and a reader should see the number.
const ROW_ALIGNMENT: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

/// How many bytes one entry of the table is.
const ENTRY: u32 = 4;

/// The device's copy of a [`TileTable`], and the parameter block beside it.
///
/// A texture rather than a storage buffer, for three reasons that all point the
/// same way. It is indexed the way the table is -- a page in two dimensions, a
/// source in the third -- so the shader's index is a `textureLoad` rather than a
/// multiply and an add. It needs no storage-buffer support, which is the one
/// binding a downlevel target may not have. And a texture's bounds are checked
/// by the device, so a page past the edge reads zero instead of somebody else's
/// memory.
#[derive(Debug)]
pub(crate) struct Table {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    params: wgpu::Buffer,
    side: u32,
    layers: u32,
    scratch: Vec<u8>,
}

impl Table {
    /// An empty table: one page, one layer, every entry absent.
    ///
    /// A texture with a zero dimension is not a texture, so a cache with nothing
    /// registered still holds the smallest one there is. Its single entry is
    /// zero, which is slot zero rather than the absent sentinel -- and that is
    /// harmless because the parameter block says no sources, so
    /// `corvid_tile_lookup` answers "nothing here" before it ever loads a word.
    pub(crate) fn new(device: &wgpu::Device, label: &str) -> Self {
        let texture = table_texture(device, label, 1, 1);
        let view = table_view(&texture);
        Self {
            texture,
            view,
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&alloc::format!("{label}.params")),
                size: Params::BYTES,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            side: 1,
            layers: 1,
            scratch: Vec::new(),
        }
    }

    /// The view the bind group holds.
    pub(crate) const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// The parameter buffer the bind group holds.
    pub(crate) const fn params(&self) -> &wgpu::Buffer {
        &self.params
    }

    /// Write a table and its parameters, answering whether the texture was
    /// replaced -- which is when the bind group has to be built again.
    ///
    /// # Errors
    ///
    /// [`CacheError::TooManySources`] for a table with more layers than the
    /// parameter block holds, and [`CacheError::TableTooWide`] for one wider in
    /// pages than a texture on this device reaches. Neither is reachable from a
    /// configuration [`TileConfig::validate`](corvid_image::TileConfig::validate)
    /// accepted on a device with the limits this workspace opens; both are here
    /// because the alternative is a driver error with no source line on it.
    pub(crate) fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        table: &TileTable,
        atlas: &Atlas,
        label: &str,
    ) -> Result<bool, CacheError> {
        let side = table.side().max(1);
        let layers = table.layers().max(1);
        if table.layers() > crate::shader::MAX_SOURCES {
            return Err(CacheError::TooManySources {
                wanted: crate::shader::MAX_SOURCES,
                given: table.layers(),
            });
        }
        let max = device.limits().max_texture_dimension_2d;
        if side > max || layers > device.limits().max_texture_array_layers {
            return Err(CacheError::TableTooWide { given: side, max });
        }

        queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&Params::new(table, atlas)),
        );

        let remade = side != self.side || layers != self.layers;
        if remade {
            self.texture = table_texture(device, label, side, layers);
            self.view = table_view(&self.texture);
            self.side = side;
            self.layers = layers;
        }
        self.upload(queue, table, side, layers);
        Ok(remade)
    }

    /// Stage the words with padded rows and hand them to the queue.
    ///
    /// The padding is what makes an arbitrary table side legal: a row of the
    /// copy has to be a multiple of [`ROW_ALIGNMENT`], and a table one page wide
    /// is four bytes. The gap between rows is never read, so it is left as
    /// whatever the scratch buffer held.
    fn upload(&mut self, queue: &wgpu::Queue, table: &TileTable, side: u32, layers: u32) {
        let stride = (side * ENTRY).div_ceil(ROW_ALIGNMENT) * ROW_ALIGNMENT;
        let rows = (side as usize) * (layers as usize);
        self.scratch.clear();
        self.scratch.resize(stride as usize * rows, 0);
        let words = table.words();
        for row in 0..rows {
            let source = words.get(row * side as usize..(row + 1) * side as usize);
            let target = self
                .scratch
                .get_mut(row * stride as usize..row * stride as usize + side as usize * 4);
            let (Some(source), Some(target)) = (source, target) else {
                continue;
            };
            target.copy_from_slice(bytemuck::cast_slice(source));
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.scratch,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(side),
            },
            wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: layers,
            },
        );
    }
}

/// The texture a table of `side` pages and `layers` sources lives in.
fn table_texture(device: &wgpu::Device, label: &str, side: u32, layers: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&alloc::format!("{label}.table")),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // Signed because a `TileEntry`'s word is, and because nothing filters
        // it: a lookup table between two texels is not a lookup table.
        format: wgpu::TextureFormat::R32Sint,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// The array view of it, which is what the shader binds even for one layer.
fn table_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}
