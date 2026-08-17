//! Building the tile atlas's mip chain on the device.
//!
//! The seam against `cache.rs` is that nothing here decides *which* tiles need
//! reducing. It is handed a list of slots and records the passes; the cache is
//! what remembers that a slot was written this frame.
//!
//! The design asks for as much of the image work as possible on the GPU, and
//! this is the largest piece of it: a box filter over a 256-texel tile is
//! eight levels of reduction, and a frame that uploads a hundred tiles is
//! eight hundred of them. A CPU doing that is a CPU doing nothing else.

use alloc::vec::Vec;

use corvid_image::TileSlot;

use crate::atlas::Atlas;
use crate::shader::MIPS_WGSL;

/// The pipeline and the views a mip chain is built through.
///
/// Every view and every bind group is made once, at construction. There is one
/// destination view per layer per level and one bind group per layer per level
/// below the last, which for the minimum specification -- one layer, nine
/// levels -- is eighteen objects. The alternative is making them per frame,
/// which is a device allocation and a free on the path a streamer runs every
/// time it uploads anything.
#[derive(Debug)]
pub(crate) struct Mips {
    pipeline: wgpu::RenderPipeline,
    /// Indexed `layer * (levels - 1) + (level - 1)`: what to draw into at
    /// `level`, and what to read from at `level - 1`.
    steps: Vec<(wgpu::TextureView, wgpu::BindGroup)>,
    levels: u32,
}

impl Mips {
    /// The pipeline, the views and the bind groups for this atlas.
    pub(crate) fn new(
        device: &wgpu::Device,
        texture: &wgpu::Texture,
        atlas: &Atlas,
        format: wgpu::TextureFormat,
        label: &str,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(MIPS_WGSL.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    // Read with `textureLoad`, so nothing filters it and the
                    // format need not be filterable to be reduced.
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
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
                    // No blend: a reduced texel replaces whatever was there.
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let levels = atlas.mip_levels();
        let mut steps = Vec::new();
        for layer in 0..atlas.layers() {
            for level in 1..levels {
                let target = texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some(label),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                });
                // One layer, one level: the source is a different subresource
                // from the target, which is what lets both be in one pass.
                let source = texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some(label),
                    dimension: Some(wgpu::TextureViewDimension::D2Array),
                    base_mip_level: level - 1,
                    mip_level_count: Some(1),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                });
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(label),
                    layout: &layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&source),
                    }],
                });
                steps.push((target, group));
            }
        }
        Self {
            pipeline,
            steps,
            levels,
        }
    }

    /// Record the reduction of every slot in `slots`.
    ///
    /// One pass per layer per level, with a scissor and a three-vertex draw per
    /// tile inside it -- so a hundred tiles in one layer is eight passes and
    /// eight hundred draws, rather than eight hundred passes. The order is by
    /// level within a layer because level `n` reads level `n - 1`, and a pass
    /// that ran out of order would reduce a level that had not been written.
    pub(crate) fn record(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        atlas: &Atlas,
        slots: &[TileSlot],
    ) {
        for layer in 0..atlas.layers() {
            let tiles: Vec<[u32; 2]> = slots
                .iter()
                .filter_map(|slot| atlas.locate(*slot))
                .filter(|(found, _)| *found == layer)
                .map(|(_, origin)| origin)
                .collect();
            if tiles.is_empty() {
                continue;
            }
            for level in 1..self.levels {
                let index = (layer * (self.levels - 1) + level - 1) as usize;
                let Some((target, source)) = self.steps.get(index) else {
                    continue;
                };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("corvid_image_render.mips"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            // Load, not clear: the tiles this pass does not
                            // scissor to are other tiles' mips and are still
                            // wanted.
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, source, &[]);
                let side = (atlas.tile_size() >> level).max(1);
                for origin in &tiles {
                    pass.set_scissor_rect(origin[0] >> level, origin[1] >> level, side, side);
                    pass.draw(0..3, 0..1);
                }
            }
        }
    }
}
