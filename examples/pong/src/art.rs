//! The drawing half: one pipeline, one vertex buffer, and no camera.
//!
//! A court seen from above is rectangles, so this file transforms them to clip
//! space on the CPU and writes one buffer per frame. There is no depth
//! attachment, no uniform block and no matrix in a shader — eleven quads is
//! fewer numbers than a uniform buffer's worth of matrix, and what it buys is
//! that the whole renderer fits on two screens and says exactly what it draws.
//!
//! Everything here is behind the `render` feature, because `wgpu` is most of
//! what a graphics stack weighs and the same game has to build on a machine
//! that has none.

use core::time::Duration;

use corvid::{Extent, Extract, Extracting, Factor16, Factor32, I16F16, Render, Target, Time};

use crate::{
    play::FLASH,
    table::{Contact, Court, Level, SEATS, Table},
};

/// A frame's alpha as the [`Factor32`] every `lerp` in the maths stack takes.
///
/// It used to live in `corvid_present`, which is gone; this is its one
/// remaining caller. Interpolation is the GPU's now for a game that has a
/// shader to do it in, and this one builds its vertices on the CPU.
///
/// # Why the multiply and not a shift
///
/// Both factor types are `UNORM`: a stored `v` denotes `v / MAX`, so
/// `Factor16::MAX` is exactly `1.0` and so is `Factor32::MAX`. The exact
/// widening is `v × 65537`, because `u32::MAX == u16::MAX × 65537`.
///
/// `v << 16` is the conversion that looks right and is not: it maps
/// `Factor16::MAX` to `0xffff_0000`, which is `1.0 - 1/65536`, so a display
/// frame sitting exactly on a tick draws the ball a sixty-five-thousandth short
/// of where the simulation put it.
#[allow(
    clippy::cast_lossless,
    reason = "the cast widens u16 to u32 and cannot lose anything; `u32::from` is not a const fn and this being const is what lets a fixed alpha fold into a constant"
)]
const fn weight(alpha: Factor16) -> Factor32 {
    Factor32::from_bits(alpha.to_bits() as u32 * 65_537)
}

/// What the court is cleared to.
const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.043,
    g: 0.055,
    b: 0.078,
    a: 1.0,
};

/// The court's floor and its markings.
const LINES: [f32; 4] = [0.16, 0.20, 0.26, 1.0];

/// Seat zero's paddle, which defends `-x`.
const LEFT: [f32; 4] = [0.98, 0.62, 0.20, 1.0];

/// Seat one's paddle, which defends `+x`.
const RIGHT: [f32; 4] = [0.36, 0.80, 0.92, 1.0];

/// The ball.
const BALL: [f32; 4] = [0.95, 0.96, 0.98, 1.0];

/// What the ball flashes to on the tick a paddle plays it.
const STRUCK: [f32; 4] = [1.0, 0.98, 0.72, 1.0];

/// How much of the court's height the score pips take, and how far in from the
/// top they sit.
const PIP: f32 = 0.055;

/// One corner of one rectangle.
///
/// Position is already in clip space, so the vertex stage is a pass-through and
/// the aspect correction happens once per frame on the CPU rather than once per
/// vertex on the device.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
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

    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        (): (),
    ) -> Self {
        Self::setup(device, queue, format)
    }

    fn configure(&mut self, (): ()) {}

    fn draw(
        &mut self,
        target: Target<'_>,
        _camera: &corvid::Camera,
        _loading: Option<corvid::Loading<'_, Level>>,
        _time: Time,
        alpha: corvid::Factor16,
    ) {
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
        // exceed it — the score is clamped to sixteen pips a side — but a
        // `write_buffer` past the end is a validation error rather than a wrong
        // picture, so the truncation is here rather than assumed.
        graphics.building.truncate(VERTICES);

        target.queue.write_buffer(
            &graphics.vertices,
            0,
            bytemuck::cast_slice(&graphics.building),
        );

        let mut pass = target
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
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

/// Court metres to clip space, with the court's shape preserved whatever the
/// window's is.
///
/// The court is letterboxed rather than stretched: a window twice as wide as it
/// should be gets bars at the sides, because a pong court that changes
/// proportion with the window would change what a shot across it looks like.
struct Space {
    /// What one metre is in clip space along `x`.
    scale_x: f32,
    /// And along `y`.
    scale_y: f32,
}

impl Space {
    /// The mapping for this court in this target.
    ///
    /// The pixel counts become `f32` here, which is a narrowing the compiler is
    /// right to mention and is exactly right for what it is used for: a window
    /// wider than sixteen million pixels does not exist, and what this produces
    /// is a scale a rasteriser will round to a pixel anyway.
    #[allow(
        clippy::cast_precision_loss,
        reason = "a window's width in pixels is far inside what an f32 counts exactly, and the result is a scale that ends up rounded to a pixel"
    )]
    fn new(court: &Court, size: Extent) -> Self {
        let half_x = court.half.x().to_f32().max(f32::EPSILON);
        let half_y = court.half.y().to_f32().max(f32::EPSILON);
        // A margin, so the court's own edge is visible rather than flush with
        // the window's.
        let fill = 0.94;
        let width = size.width.max(1) as f32;
        let height = size.height.max(1) as f32;
        let per_metre = (fill / half_x * 0.5 * width).min(fill / half_y * 0.5 * height);
        Self {
            scale_x: per_metre / (0.5 * width),
            scale_y: per_metre / (0.5 * height),
        }
    }

    /// One rectangle, given its centre and half-extents in court metres.
    fn quad(&self, into: &mut Vec<Vertex>, at: [f32; 2], half: [f32; 2], tint: [f32; 4]) {
        let (cx, cy) = (at[0] * self.scale_x, at[1] * self.scale_y);
        let (hx, hy) = (half[0] * self.scale_x, half[1] * self.scale_y);
        let corners = [
            [cx - hx, cy - hy],
            [cx + hx, cy - hy],
            [cx + hx, cy + hy],
            [cx - hx, cy - hy],
            [cx + hx, cy + hy],
            [cx - hx, cy + hy],
        ];
        into.extend(corners.map(|at| Vertex { at, tint }));
    }
}

/// Every rectangle in the picture, in the order they are drawn.
/// The two states, the flash and the weight: everything `paint` reads.
struct Painted {
    previous: Table,
    current: Table,
    since_goal: f32,
    alpha: corvid::Factor16,
}

fn paint(into: &mut Vec<Vertex>, space: &Space, frame: &Painted, court: &Court) {
    let half_x = court.half.x().to_f32();
    let half_y = court.half.y().to_f32();
    let edge = 0.06;

    // The court: two side lines and the net down the middle. Flashed after a
    // goal, which is the one thing the view is for.
    let glow = 1.0 - (frame.since_goal / FLASH).clamp(0.0, 1.0);
    let lines = [
        LINES[0] + glow * 0.5,
        LINES[1] + glow * 0.45,
        LINES[2] + glow * 0.4,
        1.0,
    ];
    space.quad(into, [0.0, half_y], [half_x, edge], lines);
    space.quad(into, [0.0, -half_y], [half_x, edge], lines);
    space.quad(into, [0.0, 0.0], [edge * 0.5, half_y], lines);

    // The paddles, at the ends they defend. Interpolated between the two states
    // the display sits between, which is what keeps a thirty-hertz simulation
    // from looking like one.
    let alpha = weight(frame.alpha);
    for seat in 0..SEATS {
        let (Some(before), Some(now)) = (
            frame.previous.paddles.get(seat),
            frame.current.paddles.get(seat),
        ) else {
            continue;
        };
        let at = before.at.lerp(now.at, alpha).to_f32();
        let tint = if seat == 0 { LEFT } else { RIGHT };
        space.quad(
            into,
            // `centre`, not `face`: the face is the plane the ball bounces off,
            // which is this rectangle's court-facing edge rather than a line
            // through the middle of it.
            [court.centre(seat).to_f32(), at],
            [court.paddle.x().to_f32(), court.paddle.y().to_f32()],
            tint,
        );
    }

    // The ball, which is not drawn at all while it is waiting to be served —
    // the state says so, and a ball parked at the centre for a second would
    // read as a ball that had stopped working.
    if frame.current.serve == 0 {
        let at = shown(frame, alpha);
        let struck = matches!(frame.current.contact, Some(Contact::Paddle { .. }));
        let tint = if struck { STRUCK } else { BALL };
        let size = court.ball.to_f32();
        space.quad(into, at, [size, size], tint);
    }

    // The score, as a pip per point along the top of each half.
    for seat in 0..SEATS {
        let Some(score) = frame.current.scores.get(seat) else {
            continue;
        };
        let side = if seat == 0 { -1.0 } else { 1.0 };
        let tint = if seat == 0 { LEFT } else { RIGHT };
        let pip = half_y * PIP;
        for point in 0..(*score).min(16) {
            let along = side * (f32::from(point) * pip).mul_add(-3.0, half_x * 0.5);
            space.quad(into, [along, half_y * 0.8], [pip, pip], tint);
        }
    }
}

/// Where the ball is for the frame being displayed.
///
/// The interpolation belongs to the client and never to the simulation, which
/// is the same rule `examples/hello` states: `weight` is exact at both ends, so
/// this is `previous` at zero and `current` at one, bit for bit.
const fn shown(frame: &Painted, alpha: Factor32) -> [f32; 2] {
    let (before, now) = (frame.previous.ball.at, frame.current.ball.at);
    [
        before.x().lerp(now.x(), alpha).to_f32(),
        before.y().lerp(now.y(), alpha).to_f32(),
    ]
}

/// The ball's position at a frame, for whoever wants it in court metres.
///
/// Public because a test asserts that the two ends of the interpolation are the
/// two states exactly, which is the obligation `Render::draw` is held to.
#[must_use]
pub fn ball_at(previous: &Table, current: &Table, alpha: Factor16) -> [I16F16; 2] {
    let frame = Painted {
        previous: previous.clone(),
        current: current.clone(),
        since_goal: 0.0,
        alpha,
    };
    let alpha = weight(frame.alpha);
    let (before, now) = (frame.previous.ball.at, frame.current.ball.at);
    [
        before.x().lerp(now.x(), alpha),
        before.y().lerp(now.y(), alpha),
    ]
}

/// What the court looks like with nothing on it, for a caller that wants the
/// state a picture is drawn from without a device to draw it on.
#[must_use]
pub const fn empty() -> Table {
    Table {
        ball: crate::table::Ball {
            at: corvid::FinePoint::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO),
            velocity: corvid::FinePoint::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO),
        },
        paddles: [crate::table::Paddle { at: I16F16::ZERO }; SEATS],
        scores: [0; SEATS],
        serve: 0,
        towards: true,
        contact: None,
        now: corvid::Tick::ZERO,
        over: None,
    }
}
