//! A device, a plan streamed onto it, and a frame read back off it.
//!
//! The seam against the test that uses it is assertion: nothing here compares
//! anything. It builds a cache, performs a plan against it, and draws frames
//! through this crate's own WGSL; what those frames have to say is
//! `device.rs`'s.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "a frame index and a mip level are small whole numbers, and every float here is one of them turned into the coordinate a shader was handed"
)]

use corvid_image::{
    Image, PixelFormat, SourceId, SourceView, TileConfig, TileKey, TilePlan, TilePlanner,
    VramBudget, extent,
};
use corvid_image_render::{TileCache, WGSL};
use corvid_render::{Extent, Renderer};

/// How big the frames are. Small, because every pixel is compared against an
/// answer computed on the CPU and nothing here is about resolution.
pub(crate) const SIZE: Extent = Extent::new(64, 64);

/// A tiny configuration, so the textures are kilobytes and the mip chain is
/// five levels rather than nine. Sixteen is the smallest tile
/// [`TileConfig::validate`] accepts.
pub(crate) const CONFIG: TileConfig = TileConfig {
    tile_size: 16,
    max_tiles: 32,
    max_sources: 4,
    max_image_size: 1024,
};

/// Enough memory for every tile of both sources, so nothing below is about
/// eviction under pressure -- `corvid_image`'s own tests cover that, and a
/// degraded plan here would make the expected frame depend on the ranking.
pub(crate) const BUDGET: VramBudget = VramBudget::new(8 << 20);

/// A plate whose edge tiles are partial: four pages by three, with the last
/// column twelve texels wide and the last row eight tall.
pub(crate) const PLATE: SourceId = SourceId(0);

/// A second source whose tiles are all whole, so a mip read of one is the
/// colour it was filled with and nothing else.
pub(crate) const SEAL: SourceId = SourceId(1);

/// The test's own shader, with this crate's addressing concatenated onto it --
/// which is exactly what a game does with [`WGSL`].
pub(crate) const PROBE: &str = "
struct Probe { source: u32, mode: u32, lod: f32, pad: f32 };
@group(0) @binding(0) var<uniform> probe: Probe;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(index / 2u) * 4.0 - 1.0;
    let y = f32(index & 1u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = position.xy / 64.0;
    if probe.mode == 0u {
        let hit = corvid_tile_lookup(probe.source, uv);
        var present = 0.0;
        if hit.present { present = 1.0; }
        return vec4<f32>(
            f32(hit.texel.x) / 255.0,
            f32(hit.texel.y) / 255.0,
            f32(hit.level) / 255.0,
            present,
        );
    }
    if probe.mode == 1u {
        return corvid_tile_sample(probe.source, uv);
    }
    return corvid_tile_sample_level(probe.source, uv, probe.lod);
}
";

/// What a tile of `key` is filled with: its own coordinates, so a frame drawn
/// from it says which tile it came from.
///
/// Linear rather than sRGB on purpose. The offscreen target is `Rgba8Unorm`, so
/// a byte written into a tile comes back out of a frame as itself and every
/// comparison below is exact; through an sRGB texture it would be a byte
/// decoded, filtered and encoded again, and the test would need a tolerance
/// that could hide a real error.
pub(crate) fn painted(key: TileKey, width: u32, height: u32) -> Image {
    let texel = [
        u8::try_from(key.x).unwrap_or(u8::MAX),
        u8::try_from(key.y).unwrap_or(u8::MAX),
        key.level,
        u8::MAX,
    ];
    let texels = texel.repeat((width * height) as usize);
    Image::new(extent(width, height), PixelFormat::RGBA8, texels).expect("a whole number of texels")
}

/// How big the tile `key` names is, which is smaller than a slot at the right
/// and bottom edges of a source.
pub(crate) fn tile_extent(key: TileKey, source: corvid_image::Extent) -> (u32, u32) {
    let side = CONFIG.tile_size << key.level;
    let across = source
        .width
        .saturating_sub(u32::from(key.x) * side)
        .min(side);
    let down = source
        .height
        .saturating_sub(u32::from(key.y) * side)
        .min(side);
    // A tile at a coarse level covers more ground than it has texels, so its
    // own size is the covered ground scaled down by the level.
    (
        (across >> key.level).clamp(1, CONFIG.tile_size),
        (down >> key.level).clamp(1, CONFIG.tile_size),
    )
}

/// The pixel at `x`, `y` of a read-back frame.
pub(crate) fn pixel(image: &corvid_render::Image, x: u32, y: u32) -> [u8; 4] {
    let start = ((y * image.size.width + x) * 4) as usize;
    [
        image.pixels[start],
        image.pixels[start + 1],
        image.pixels[start + 2],
        image.pixels[start + 3],
    ]
}

/// Everything the frames are drawn with.
pub(crate) struct Probing {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
}

impl Probing {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        cache: &TileCache,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("probe"),
            source: wgpu::ShaderSource::Wgsl(format!("{WGSL}\n{PROBE}").into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("probe"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("probe"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("probe"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("probe"),
            bind_group_layouts: &[Some(&layout), Some(cache.layout())],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("probe"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            uniform,
            binding,
        }
    }

    /// One frame of one mode, read back.
    pub(crate) fn frame(
        &self,
        renderer: &mut Renderer,
        cache: &TileCache,
        source: SourceId,
        mode: u32,
        lod: f32,
    ) -> corvid_render::Image {
        renderer
            .frame(|target, encoder| {
                target.queue.write_buffer(
                    &self.uniform,
                    0,
                    bytemuck::cast_slice(&[u32::from(source.0), mode, lod.to_bits(), 0]),
                );
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("probe"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.binding, &[]);
                pass.set_bind_group(1, cache.bind_group(), &[]);
                pass.draw(0..3, 0..1);
            })
            .expect("an offscreen renderer always has somewhere to draw");
        renderer.read_back().expect("the frame comes back")
    }
}

/// The planner, the plan and the cache, with every upload performed.
pub(crate) fn streamed(renderer: &mut Renderer) -> (TilePlan, TileCache) {
    let mut planner = TilePlanner::new(CONFIG).expect("the configuration is a legal one");
    let plate = planner
        .register(extent(60, 40), PixelFormat::RGBA8)
        .expect("a 60 by 40 plate fits");
    let seal = planner
        .register(extent(32, 32), PixelFormat::RGBA8)
        .expect("a 32 by 32 seal fits");
    assert_eq!((plate, seal), (PLATE, SEAL));

    let mut cache = TileCache::new(
        renderer.device(),
        CONFIG,
        BUDGET,
        PixelFormat::RGBA8,
        "probe",
    )
    .expect("a 16-texel tile fits any device that renders at all");

    let plan = planner.plan(&[SourceView::full(plate), SourceView::full(seal)], BUDGET);
    assert!(
        !plan.is_degraded(),
        "the budget was meant to hold everything"
    );
    assert_eq!(plan.capacity(), cache.slots());
    cache.admits(&plan).expect("the plan fits this cache");

    cache.evict(plan.evictions());
    for upload in plan.uploads() {
        let source = planner
            .sources()
            .get(upload.key.source)
            .expect("a plan only names registered sources");
        let (width, height) = tile_extent(upload.key, source.extent());
        cache
            .upload(
                renderer.queue(),
                upload,
                &painted(upload.key, width, height),
            )
            .expect("a tile of the cache's own format fits its own slot");
    }
    renderer
        .frame(|_, encoder| cache.generate_mips(encoder))
        .expect("an offscreen renderer always has somewhere to draw");
    cache
        .write_table(renderer.device(), renderer.queue(), plan.table())
        .expect("a table built under the cache's own configuration");
    planner.commit(&plan);
    assert_eq!(planner.residency().len(), plan.uploads().len());
    (plan, cache)
}
