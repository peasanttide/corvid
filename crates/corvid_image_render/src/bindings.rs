//! The `wgpu` descriptors a tile cache is made of.
//!
//! The seam is that nothing here decides anything. Every number these three
//! take is one `Atlas` or `TileConfig` already chose, and gathering them in one
//! file is what makes the bind group layout and the group that fills it
//! readable against each other -- and against `tiles.wgsl`, which is the third
//! statement of the same four bindings.

use crate::atlas::Atlas;
use crate::table::Table;

/// The array texture the tiles live in.
pub(crate) fn tile_texture(
    device: &wgpu::Device,
    label: &str,
    atlas: &Atlas,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    let (width, height) = atlas.layer_extent();
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&alloc::format!("{label}.tiles")),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: atlas.layers(),
        },
        mip_level_count: atlas.mip_levels(),
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        // `RENDER_ATTACHMENT` is the mip chain: the reduction draws into the
        // level below, which is the only way to build one without a compute
        // shader writing a storage texture -- and there is no sRGB storage
        // texture on any device, which is what the archive is made of.
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// The layout of the group `tiles.wgsl` declares.
pub(crate) fn bind_group_layout(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&alloc::format!("{label}.tiles")),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Sint,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// That group, filled.
pub(crate) fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    table: &Table,
    tiles: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&alloc::format!("{label}.tiles")),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: table.params().as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(table.view()),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(tiles),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
