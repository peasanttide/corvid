//! The drawing half: one pipeline, one vertex buffer, and no camera.
//!
//! A court seen from above is rectangles, so this file transforms them to clip
//! space on the CPU and writes one buffer per frame. There is no depth
//! attachment, no uniform block and no matrix in a shader -- eleven quads is
//! fewer numbers than a uniform buffer's worth of matrix, and what it buys is
//! that the whole renderer fits on two screens and says exactly what it draws.
//!
//! Everything here is behind the `render` feature, because `wgpu` is most of
//! what a graphics stack weighs and the same game has to build on a machine
//! that has none.

use core::time::Duration;

use corvid::{Drawing, Extract, Extracting, Opened, Render};

use crate::{
    play::FLASH,
    table::{Court, SEATS, Table},
};

/// What the court is cleared to.
const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.043,
    g: 0.055,
    b: 0.078,
    a: 1.0,
};

/// The court's floor and its markings.
pub(super) const LINES: [f32; 4] = [0.16, 0.20, 0.26, 1.0];

/// Seat zero's paddle, which defends `-x`.
pub(super) const LEFT: [f32; 4] = [0.98, 0.62, 0.20, 1.0];

/// Seat one's paddle, which defends `+x`.
pub(super) const RIGHT: [f32; 4] = [0.36, 0.80, 0.92, 1.0];

/// The ball.
pub(super) const BALL: [f32; 4] = [0.95, 0.96, 0.98, 1.0];

/// What the ball flashes to on the tick a paddle plays it.
pub(super) const STRUCK: [f32; 4] = [1.0, 0.98, 0.72, 1.0];

/// How much of the court's height the score pips take, and how far in from the
/// top they sit.
pub(super) const PIP: f32 = 0.055;

/// One corner of one rectangle.
///
/// Position is already in clip space, so the vertex stage is a pass-through and
/// the aspect correction happens once per frame on the CPU rather than once per
/// vertex on the device.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Vertex {
    /// Where, in clip space.
    at: [f32; 2],
    /// What colour.
    tint: [f32; 4],
}

impl Vertex {
    /// How the vertex stage reads one of these.
    const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: size_of::<Self>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 8,
                shader_location: 1,
            },
        ],
    };
}

/// The most rectangles one frame can hold: the court's markings, two paddles,
/// the ball, and a pip per point per seat.
///
/// A fixed capacity rather than a buffer that grows, because the number is
/// knowable and a `write_buffer` into a buffer sized once at startup is the
/// whole of this renderer's per-frame allocation story.
const QUADS: usize = 4 + SEATS + 1 + 2 * 16;

/// Six vertices to a rectangle, because two triangles.
const VERTICES: usize = QUADS * 6;

/// Everything built once.
#[derive(Debug)]
pub struct Graphics {
    /// The one pipeline.
    pipeline: wgpu::RenderPipeline,
    /// The vertices, rewritten every frame.
    vertices: wgpu::Buffer,
    /// The CPU side of the same, kept so a frame allocates nothing.
    building: Vec<Vertex>,
    /// The two states the shader interpolates between, and the court they are
    /// played on. Written by `extract`, read by `draw`.
    ///
    /// This is where `View` went. A flash after a goal is a cosmetic value that
    /// only the picture wants, so it lives in the thing that draws the picture
    /// rather than in a struct three functions could write through.
    previous: Table,
    /// The newer of the pair.
    current: Table,
    /// The court, so `draw` needs nothing but this.
    court: Court,
    /// Seconds since the last goal, saturating at [`FLASH`].
    since_goal: f32,
    /// The score the last extraction saw, so a goal is noticed by the score
    /// changing rather than by reading a `Contact` a display may see zero times
    /// or five.
    scores: [u16; SEATS],
    /// When the last extraction happened, so this one knows how long ago.
    seen: Duration,
}

/// Built once, by the type that holds it.
impl Graphics {
    fn setup(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pong.flat"),
            source: wgpu::ShaderSource::Wgsl(include_str!("pong.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pong.flat"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pong.flat"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(Vertex::LAYOUT)],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // Nothing here has a back: every rectangle is drawn in the
                // order it should appear, in one pass, with no depth test.
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            vertices: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("pong.vertices"),
                size: (size_of::<Vertex>() * VERTICES) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            building: Vec::with_capacity(VERTICES),
            previous: Table::default(),
            current: Table::default(),
            court: crate::court(),
            since_goal: FLASH,
            scores: [0; SEATS],
            seen: Duration::ZERO,
        }
    }
}

impl Extract<Table> for Graphics {
    /// At most once per displayed frame, for the settled newest state.
    fn extract(&mut self, extracting: Extracting<'_, Table>) {
        // The pair shifts only when the state does. A frame that saw no tick
        // extracts nothing new, and the shader keeps lerping between the same
        // two.
        if extracting.state.now != self.current.now {
            self.previous = core::mem::replace(&mut self.current, extracting.state.clone());
        }
        self.court = extracting.level.clone();

        let dt = extracting.time.elapsed.saturating_sub(self.seen);
        self.seen = extracting.time.elapsed;
        if extracting.state.scores == self.scores {
            self.since_goal = (self.since_goal + dt.as_secs_f32()).min(FLASH);
        } else {
            self.scores = extracting.state.scores;
            self.since_goal = 0.0;
        }
    }
}

impl Render<Table> for Graphics {
    type Config = ();

    fn new(opened: Opened<'_>, (): ()) -> Self {
        Self::setup(opened.device, opened.queue, opened.format)
    }

    fn configure(&mut self, (): ()) {}

    fn draw(&mut self, drawing: Drawing<'_>, encoder: &mut wgpu::CommandEncoder) {
        let target = drawing.target;
        let alpha = drawing.alpha;
        let graphics = self;
        let court = graphics.court.clone();
        let space = Space::new(&court, target.size);

        let painted = Painted {
            previous: graphics.previous.clone(),
            current: graphics.current.clone(),
            since_goal: graphics.since_goal,
            alpha,
        };
        graphics.building.clear();
        paint(&mut graphics.building, &space, &painted, &court);
        // The buffer is sized for `VERTICES` and the painter above cannot
        // exceed it -- the score is clamped to sixteen pips a side -- but a
        // `write_buffer` past the end is a validation error rather than a wrong
        // picture, so the truncation is here rather than assumed.
        graphics.building.truncate(VERTICES);

        target.queue.write_buffer(
            &graphics.vertices,
            0,
            bytemuck::cast_slice(&graphics.building),
        );

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pong.court"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(BACKGROUND),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&graphics.pipeline);
        pass.set_vertex_buffer(0, graphics.vertices.slice(..));
        let drawn = u32::try_from(graphics.building.len()).unwrap_or(0);
        pass.draw(0..drawn, 0..1);
    }
}

mod paint;

pub use paint::{ball_at, empty};

use paint::{Painted, Space, paint};
