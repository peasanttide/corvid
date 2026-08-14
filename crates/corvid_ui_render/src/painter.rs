//! The device half: two pipelines, two instance buffers, and the upload.
//!
//! This is the only file in the crate that names a `wgpu` resource. Everything
//! it draws was decided by `instance.rs` and `batch.rs`, which have no device
//! in them at all.

use alloc::vec::Vec;

use corvid_fixed::I16F16;
use corvid_render::Extent;
use corvid_ui::{Painted, Rect};

use crate::batch::count;
use crate::{Atlas, Batch, GlyphInstance, QUAD, RectInstance, SHADER, batches, scissor};

/// Everything built once: two pipelines, two instance buffers, and the binding
/// the glyph atlas is sampled through.
///
/// Owned by a game's `Render::Graphics`, built in `Render::setup`, and handed
/// one [`Painted`] a frame -- all three on the one trait, which is the trait a
/// device belongs to.
#[derive(Debug)]
pub struct Painter {
    rects: wgpu::RenderPipeline,
    glyphs: wgpu::RenderPipeline,
    binding: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    rect_buffer: wgpu::Buffer,
    rect_capacity: u32,
    glyph_buffer: wgpu::Buffer,
    glyph_capacity: u32,
    batches: Vec<Batch>,
    clips: Vec<Rect>,
    size: Rect,
}

impl Painter {
    /// The instance count a new painter's buffers start at.
    ///
    /// A menu is about this many rectangles, so the common case allocates
    /// once, at start-up, and never again.
    const INITIAL: u32 = 64;

    /// Build the pipelines.
    ///
    /// `atlas` is the coverage texture the glyph pipeline samples: one channel,
    /// where 1.0 is ink. `sampler` is how it is filtered -- linear for a scalable
    /// face, nearest for a bitmap one.
    #[must_use]
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        atlas: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("corvid_ui"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("corvid_ui.viewport"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("corvid_ui"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("corvid_ui"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(atlas),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("corvid_ui"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        Self {
            rects: pipeline(
                device,
                &pipeline_layout,
                &shader,
                format,
                ("rect_vertex", "rect_fragment"),
                RectInstance::LAYOUT,
            ),
            glyphs: pipeline(
                device,
                &pipeline_layout,
                &shader,
                format,
                ("glyph_vertex", "glyph_fragment"),
                GlyphInstance::LAYOUT,
            ),
            binding,
            uniform,
            rect_buffer: instances(device, "rects", u64::from(Self::INITIAL) * 64),
            rect_capacity: Self::INITIAL,
            glyph_buffer: instances(device, "glyphs", u64::from(Self::INITIAL) * 48),
            glyph_capacity: Self::INITIAL,
            batches: Vec::new(),
            clips: Vec::new(),
            size: Rect::ZERO,
        }
    }

    /// Upload one frame's paint data.
    ///
    /// Grows the instance buffers when it must and never shrinks them, so a
    /// steady UI uploads and does not allocate. An empty [`Painted`] records no
    /// batches, which is no draw calls at all.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        painted: &Painted,
        atlas: &dyn Atlas,
    ) {
        let rects: Vec<RectInstance> = painted.rects.iter().map(RectInstance::from).collect();
        let glyphs: Vec<GlyphInstance> = painted
            .glyphs
            .iter()
            .map(|glyph| GlyphInstance::new(glyph, atlas))
            .collect();

        self.batches = batches(painted);
        self.clips.clear();
        self.clips.extend_from_slice(&painted.clips);
        self.size = painted.size;

        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::cast_slice(&[
                painted.size.width.to_f32(),
                painted.size.height.to_f32(),
                0.0,
                0.0,
            ]),
        );
        if count(rects.len()) > self.rect_capacity {
            self.rect_capacity = grown(self.rect_capacity, count(rects.len()));
            self.rect_buffer = instances(device, "rects", u64::from(self.rect_capacity) * 64);
        }
        if count(glyphs.len()) > self.glyph_capacity {
            self.glyph_capacity = grown(self.glyph_capacity, count(glyphs.len()));
            self.glyph_buffer = instances(device, "glyphs", u64::from(self.glyph_capacity) * 48);
        }
        if !rects.is_empty() {
            queue.write_buffer(&self.rect_buffer, 0, bytemuck::cast_slice(&rects));
        }
        if !glyphs.is_empty() {
            queue.write_buffer(&self.glyph_buffer, 0, bytemuck::cast_slice(&glyphs));
        }
    }

    /// How many instances each buffer holds without being rebuilt.
    ///
    /// What a test watches to see that a steady UI has stopped allocating.
    #[must_use]
    pub const fn capacity(&self) -> (u32, u32) {
        (self.rect_capacity, self.glyph_capacity)
    }

    /// The runs the last [`upload`](Self::upload) split the layout into.
    #[must_use]
    pub fn batches(&self) -> &[Batch] {
        &self.batches
    }

    /// Record the draw into a pass the game opened.
    ///
    /// Two draws a batch: one instanced rounded rectangle, one instanced glyph
    /// quad, with a scissor between them and the one before.
    ///
    /// `viewport` is the attachment's size in physical pixels, which is not
    /// necessarily the size the layout was solved at: the vertex stage divides
    /// by [`Painted::size`], so a layout solved at one size is *stretched* onto
    /// the target. A scissor is in the target's pixels and a clip rectangle is
    /// in the layout's, so the clips are carried across the same stretch --
    /// without which a UI solved larger than its window would scissor away
    /// everything past the window's own width.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, viewport: Extent) {
        if self.batches.is_empty() || viewport.is_empty() {
            return;
        }
        pass.set_bind_group(0, &self.binding, &[]);
        for batch in &self.batches {
            let Some(clip) = self.clips.get(batch.clip as usize) else {
                continue;
            };
            let Some((x, y, width, height)) = scissor(self.stretched(*clip, viewport), viewport)
            else {
                continue;
            };
            pass.set_scissor_rect(x, y, width, height);
            if !batch.rects.is_empty() {
                pass.set_pipeline(&self.rects);
                pass.set_vertex_buffer(0, self.rect_buffer.slice(..));
                pass.draw(0..QUAD, batch.rects.clone());
            }
            if !batch.glyphs.is_empty() {
                pass.set_pipeline(&self.glyphs);
                pass.set_vertex_buffer(0, self.glyph_buffer.slice(..));
                pass.draw(0..QUAD, batch.glyphs.clone());
            }
        }
        pass.set_scissor_rect(0, 0, viewport.width, viewport.height);
    }

    /// A clip rectangle carried from the layout's own pixels into the target's.
    ///
    /// The identity when the two agree, which is the case a game that solves
    /// its layout at the size of its window is in.
    fn stretched(&self, clip: Rect, viewport: Extent) -> Rect {
        let (across, down) = (self.size.width.to_bits(), self.size.height.to_bits());
        if across <= 0 || down <= 0 {
            return clip;
        }
        // Both sides in the same units: a length's own bits are sixteen
        // fractional, and a viewport is whole pixels, so the viewport is what
        // moves.
        let along = |value: I16F16, from: i32, to: u32| {
            I16F16::from_bits(
                i32::try_from(
                    (i64::from(value.to_bits()) * (i64::from(to) << 16)) / i64::from(from),
                )
                .unwrap_or(i32::MAX),
            )
        };
        Rect::new(
            along(clip.x, across, viewport.width),
            along(clip.y, down, viewport.height),
            along(clip.width, across, viewport.width),
            along(clip.height, down, viewport.height),
        )
    }
}

/// The capacity to grow to: double, or the number asked for if that is larger.
const fn grown(capacity: u32, wanted: u32) -> u32 {
    let doubled = capacity.saturating_mul(2);
    if doubled > wanted { doubled } else { wanted }
}

/// An instance buffer of this many bytes.
fn instances(device: &wgpu::Device, what: &str, bytes: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(&format!("corvid_ui.{what}")),
        size: bytes,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// One of the two pipelines, which differ only in their entry points and their
/// instance layout.
fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    entries: (&str, &str),
    instance: wgpu::VertexBufferLayout<'static>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(entries.0),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(entries.0),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(instance)],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            // A UI quad is drawn from whichever side the layout put it on, and
            // a panel in world space is looked at from both.
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(entries.1),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
