#![doc = include_str!("../README.md")]

// Named rather than re-exported: this crate draws a `corvid_ui` layout, so it
// says so in its own imports and a game reaching for either names it directly.
use corvid_fixed::I16F16;
use corvid_ui::GlyphId;

// No `no_std`. A pipeline is created on a device, and this is the layer whose
// job that is; `corvid_ui` is the half that has no device in it and builds for
// a target with no operating system.

use corvid_render::Extent;
use corvid_ui::{Painted, PaintedGlyph, PaintedRect, Rect};

/// The shader both pipelines are built from.
const SHADER: &str = include_str!("ui.wgsl");

/// How many vertices a quad drawn as a triangle strip is.
const QUAD: u32 = 4;

/// Where each glyph is in the atlas texture.
///
/// A trait rather than a texture, because rasterising a font is a different
/// crate's job and this one only needs to know where the result landed.
pub trait Atlas {
    /// This glyph's corners in the atlas, as `[u0, v0, u1, v1]` in `0..=1`.
    ///
    /// A glyph the atlas does not hold answers a zero-area rectangle, which
    /// samples nothing and draws nothing.
    fn uv(&self, glyph: GlyphId) -> [f32; 4];

    /// How large this glyph is on the page, as a multiple of the em size.
    ///
    /// `[left, top, width, height]`, right and down from the pen position on
    /// the baseline — so `top` is normally negative, because a glyph sits
    /// above the line it is on.
    fn quad(&self, glyph: GlyphId) -> [f32; 4];
}

/// The stand-in, and public API rather than a test helper: an atlas of equal
/// cells in row-major order.
///
/// A bitmap font is exactly this, and so is the first thing anyone builds
/// while a real shaper is still being written. A glyph's number is its cell.
///
/// ```
/// use corvid_ui_render::{Atlas as _, Grid};
/// use corvid_ui::GlyphId;
///
/// // Sixteen by sixteen cells starting at the space, so the seventeenth cell
/// // after it is the second of the second row.
/// let atlas = Grid::new(16, 16, 32);
/// let [u0, v0, u1, v1] = atlas.uv(GlyphId(32 + 17));
/// assert_eq!((u0, v0), (0.0625, 0.0625));
/// assert_eq!((u1, v1), (0.125, 0.125));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Grid {
    /// Cells across.
    pub columns: u32,
    /// Cells down.
    pub rows: u32,
    /// The first glyph number the top left cell holds.
    pub first: u32,
}

impl Grid {
    /// A grid of this many cells, whose top left holds `first`.
    #[must_use]
    pub const fn new(columns: u32, rows: u32, first: u32) -> Self {
        Self {
            columns,
            rows,
            first,
        }
    }
}

impl Atlas for Grid {
    fn uv(&self, glyph: GlyphId) -> [f32; 4] {
        let cells = self.columns * self.rows;
        if self.columns == 0 || self.rows == 0 || glyph.0 < self.first {
            return [0.0; 4];
        }
        let cell = glyph.0 - self.first;
        if cell >= cells {
            return [0.0; 4];
        }
        let (width, height) = (1.0 / f64::from(self.columns), 1.0 / f64::from(self.rows));
        let (x, y) = (
            f64::from(cell % self.columns) * width,
            f64::from(cell / self.columns) * height,
        );
        [narrow(x), narrow(y), narrow(x + width), narrow(y + height)]
    }

    fn quad(&self, _glyph: GlyphId) -> [f32; 4] {
        // A cell of a bitmap font is the em square, sitting three quarters
        // above the baseline — the proportions `corvid_ui::Monospace` measures
        // with, so a layout and its glyphs agree without a second table.
        [0.0, -0.75, 1.0, 1.0]
    }
}

use corvid_float::demote as narrow;

/// The instance a rectangle becomes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RectInstance {
    /// Left, top, width and height, in physical pixels.
    pub rect: [f32; 4],
    /// What fills it, linear.
    pub fill: [f32; 4],
    /// What outlines it, linear.
    pub border: [f32; 4],
    /// Border width, corner radius, and two the shader does not read.
    pub params: [f32; 4],
}

impl RectInstance {
    /// What a pipeline is told to read these at.
    pub const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: size_of::<Self>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4
        ],
    };
}

impl From<&PaintedRect> for RectInstance {
    fn from(painted: &PaintedRect) -> Self {
        Self {
            rect: pixels(painted.rect),
            fill: painted.fill.to_linear().to_f32_array(),
            border: painted.border.to_linear().to_f32_array(),
            params: [
                painted.border_width.to_f32(),
                painted.corner.to_f32(),
                0.0,
                0.0,
            ],
        }
    }
}

/// The instance a glyph becomes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GlyphInstance {
    /// Left, top, width and height, in physical pixels.
    pub rect: [f32; 4],
    /// Its corners in the atlas, as `[u0, v0, u1, v1]`.
    pub uv: [f32; 4],
    /// What it is drawn in, linear.
    pub tint: [f32; 4],
}

impl GlyphInstance {
    /// What a pipeline is told to read these at.
    pub const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: size_of::<Self>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4],
    };

    /// One placed glyph, against the atlas that holds it.
    #[must_use]
    pub fn new(painted: &PaintedGlyph, atlas: &dyn Atlas) -> Self {
        let size = painted.size.to_f32();
        let [left, top, width, height] = atlas.quad(painted.glyph);
        Self {
            rect: [
                left.mul_add(size, painted.at.x.to_f32()),
                top.mul_add(size, painted.at.y.to_f32()),
                width * size,
                height * size,
            ],
            uv: atlas.uv(painted.glyph),
            tint: painted.tint.to_linear().to_f32_array(),
        }
    }
}

/// A rectangle as the four `f32` an instance holds.
const fn pixels(rect: Rect) -> [f32; 4] {
    [
        rect.x.to_f32(),
        rect.y.to_f32(),
        rect.width.to_f32(),
        rect.height.to_f32(),
    ]
}

/// One run of instances under one scissor rectangle.
///
/// A UI with one scroll region is two batches; a UI with fifty is fifty, and
/// that is the number to watch if a HUD ever gets slow.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Batch {
    /// Which entry of [`Painted::clips`] this run is scissored to.
    pub clip: u32,
    /// The rectangles in it.
    pub rects: core::ops::Range<u32>,
    /// The glyphs in it.
    pub glyphs: core::ops::Range<u32>,
}

/// How a painted layout splits into scissored runs.
///
/// A subtree is contiguous in tree order and a clip region is a subtree, so a
/// run of one clip index is a run in both lists at once. Within one run the
/// rectangles are drawn and then the glyphs, which is what puts a label over
/// the panel it is on.
///
/// ```
/// use corvid_ui::Painted;
/// use corvid_ui_render::batches;
///
/// // Nothing to draw is no batches, and therefore no draw calls.
/// assert!(batches(&Painted::default()).is_empty());
/// ```
#[must_use]
pub fn batches(painted: &Painted) -> Vec<Batch> {
    let mut out = Vec::new();
    let (mut rect, mut glyph) = (0, 0);
    while rect < painted.rects.len() || glyph < painted.glyphs.len() {
        let clip = painted.rects.get(rect).map_or_else(
            || painted.glyphs.get(glyph).map_or(0, |it| it.clip),
            |it| it.clip,
        );
        let first_rect = rect;
        while painted.rects.get(rect).is_some_and(|it| it.clip == clip) {
            rect += 1;
        }
        let first_glyph = glyph;
        while painted.glyphs.get(glyph).is_some_and(|it| it.clip == clip) {
            glyph += 1;
        }
        out.push(Batch {
            clip,
            rects: count(first_rect)..count(rect),
            glyphs: count(first_glyph)..count(glyph),
        });
    }
    out
}

/// An index as the `u32` a draw call takes.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a draw call's instance range is a u32, so a UI with four billion rectangles could not be drawn whatever this returned"
)]
const fn count(index: usize) -> u32 {
    index as u32
}

/// Everything built once: two pipelines, two instance buffers, and the binding
/// the glyph atlas is sampled through.
///
/// Owned by a game's `Render::Graphics`, built in `Render::setup`, and handed
/// one [`Painted`] a frame — all three on the one trait, which is the trait a
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
    /// where 1.0 is ink. `sampler` is how it is filtered — linear for a scalable
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
    /// in the layout's, so the clips are carried across the same stretch —
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

/// A clip rectangle as the scissor a pass takes, or nothing when it is off the
/// target entirely.
///
/// A scissor outside the attachment is a validation error rather than an empty
/// draw, which is why this answers `None` rather than clamping to nothing.
#[must_use]
pub fn scissor(clip: Rect, viewport: Extent) -> Option<(u32, u32, u32, u32)> {
    let whole = Rect::of(
        I16F16::from_bits(i32::try_from(viewport.width).unwrap_or(i32::MAX) << 16),
        I16F16::from_bits(i32::try_from(viewport.height).unwrap_or(i32::MAX) << 16),
    );
    let inside = clip.intersection(whole);
    if inside.width.to_bits() <= 0 || inside.height.to_bits() <= 0 {
        return None;
    }
    Some((
        whole_pixels(inside.x),
        whole_pixels(inside.y),
        whole_pixels(inside.width),
        whole_pixels(inside.height),
    ))
}

/// A length as the whole pixels a scissor is in, rounded down.
const fn whole_pixels(value: I16F16) -> u32 {
    let bits = value.to_bits();
    if bits <= 0 {
        0
    } else {
        (bits >> 16).cast_unsigned()
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
