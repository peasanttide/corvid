//! The whole path, on a machine with no display: fixed-point geometry in,
//! pixels out.
//!
//! Every test here goes through the same [`Renderer`] a window uses — the same
//! acquire, the same encoder, the same submit, the same conversion out of fixed
//! point — and differs only in where the frame lands. That is what makes these
//! tests about the renderer rather than about a second implementation of one.
//!
//! The pipeline and the shader belong to this file rather than to the crate,
//! because after the pass graph went there is no pipeline in `corvid_render`
//! at all. Bringing one is what a game does, and it is what
//! [`Graphics::new`] is: eighty lines that a game writes once and that this
//! file writes so the device path can be exercised without one.
//!
//! # When these do not run
//!
//! They need an adapter: a real GPU, or a software rasteriser such as Mesa's
//! `lavapipe`. On a machine with neither, [`Renderer::offscreen`] answers
//! `Error::NoAdapter` and each test below prints why it stopped and passes.
//! That is a deliberate hole and it is the reason `src/matrix.rs` carries its
//! own tests: the conventions a projection can get wrong are checked without a
//! device, and these check that a device does what the conventions say.
//!
//! # Why every one of them is under a deadline
//!
//! These are the only tests in the workspace that wait on something outside the
//! process. [`Renderer::read_back`] submits work and then polls the device with
//! `PollType::Wait { timeout: None }`, which has no deadline of its own, and each
//! test below opens a device of its own — so several software-rasteriser devices
//! exist at once and each of them runs worker threads.
//!
//! That wedges, and it is not rare: with all of them rendering at once, about one
//! release run in three never came back. This binary was found spinning a core
//! with its test threads parked on futexes and a driver worker at a hundred per
//! cent, half an hour after the run that started it was over, and the run that
//! started it had been reported clean.
//!
//! So there are three things here, and each covers what the one before it does
//! not. The tests take [`RENDERING`] in turn, which is what makes the wedge rare
//! rather than routine — one device at a time is the load this was ever tested
//! under. Each runs on a thread of its own under [`PATIENCE`], so a wedge is a
//! named failing test rather than silence. And [`impatience`] aborts the
//! process, because a wedged driver thread does not let it exit: the run below
//! reported its failures at the deadline and then sat there until something
//! killed it, which is the whole hang over again with a nicer message on it.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::print_stderr,
    reason = "a test that is skipped has to say so where a person running the suite will see it, and the workspace's answer everywhere else — a tracing event — needs a subscriber that a test harness does not install"
)]

use std::{
    sync::{Mutex, OnceLock, PoisonError, mpsc},
    thread,
    time::{Duration, Instant},
};

use corvid_camera::matrix;
use corvid_fixed::{Angle16, I16F16, Signed32};
use corvid_glm::Mat4;
use corvid_mesh::{Mesh, Vertex};
use corvid_mesh_render::{Uploaded, VERTEX_LAYOUT, upload};
use corvid_render::{Extent, Image, Renderer, Target};
use corvid_rotation::{FineRotation, Rotation};
use corvid_shape::Frustum;
use corvid_transform::{GlobalFineTransform, Transform};
use corvid_vector::{Direction, FinePoint, OctDirection};

/// How big the frames are. Small, because every pixel is read back and
/// compared and nothing here is about resolution.
const SIZE: Extent = Extent::new(64, 64);

/// What the pass clears to: a dark blue nothing else in these tests is.
const NOTHING: wgpu::Color = wgpu::Color {
    r: 8.0 / 255.0,
    g: 12.0 / 255.0,
    b: 40.0 / 255.0,
    a: 1.0,
};

/// The same colour as the eight-bit bytes it reads back as.
const CLEARED: [u8; 4] = [8, 12, 40, 255];

/// How far the cube reaches from its own origin: one metre, so a full-scale
/// position component is one metre and a corner sits on it.
const REACH: I16F16 = I16F16::from_f64(1.0);

/// A unit cube, flat-shaded, wound counter-clockwise seen from outside.
///
/// Twenty-four vertices rather than eight, because a face's normal belongs to
/// the face and a shared vertex would have to average the three that meet
/// there. Each face is built from a tangent and a bitangent whose cross product
/// is the outward normal, which is what makes the winding right by construction
/// rather than by six copied index lists.
///
/// `facing` decides what normal every vertex gets: the face's own outward
/// direction, or one the caller names. The second is what
/// [`the_normal_reaches_the_shader_and_is_decoded_there`] needs.
fn cube(facing: Option<OctDirection>) -> Mesh {
    /// Each face as its outward normal, a tangent and a bitangent, in that
    /// order, with `tangent × bitangent = normal`.
    const FACES: [([i32; 3], [i32; 3], [i32; 3]); 6] = [
        ([1, 0, 0], [0, 1, 0], [0, 0, 1]),
        ([-1, 0, 0], [0, 0, 1], [0, 1, 0]),
        ([0, 1, 0], [0, 0, 1], [1, 0, 0]),
        ([0, -1, 0], [1, 0, 0], [0, 0, 1]),
        ([0, 0, 1], [1, 0, 0], [0, 1, 0]),
        ([0, 0, -1], [0, 1, 0], [1, 0, 0]),
    ];

    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, along, across) in FACES {
        let base = u32::try_from(vertices.len()).unwrap();
        let outward = OctDirection::encode(Direction::new(
            Signed32::from_f64(f64::from(normal[0])),
            Signed32::from_f64(f64::from(normal[1])),
            Signed32::from_f64(f64::from(normal[2])),
        ));
        for (u, v) in [(-1, -1), (1, -1), (1, 1), (-1, 1)] {
            let mut position = [0i16; 3];
            for axis in 0..3 {
                position[axis] = i16::try_from(
                    (normal[axis] + u * along[axis] + v * across[axis]) * i32::from(Vertex::FULL),
                )
                .unwrap_or(Vertex::FULL);
            }
            vertices.push(Vertex::new(position, facing.unwrap_or(outward)));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(vertices, indices, REACH)
}

/// One cube's worth of uniform: the whole transform, a tint, and the mesh's
/// metre scale.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    clip: Mat4,
    tint: [f32; 4],
    scale: [f32; 4],
}

/// The pipeline, the layout and the mesh: what a game builds in `setup`.
struct Graphics {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    mesh: Uploaded,
    depth: wgpu::Texture,
}

impl Graphics {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, mesh: &Mesh) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cube"),
            source: wgpu::ShaderSource::Wgsl(include_str!("cube.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cube.uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(size_of::<Uniforms>() as u64),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cube"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cube"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(VERTEX_LAYOUT)],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
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
            layout,
            mesh: upload(mesh, device, "cube"),
            depth: depth_texture(device, SIZE),
        }
    }
}

/// A depth attachment the size of the target.
fn depth_texture(device: &wgpu::Device, size: Extent) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cube.depth"),
        size: wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth24Plus,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

/// A camera six metres back from the origin, facing the cube.
const fn watching() -> GlobalFineTransform {
    GlobalFineTransform::new(
        FinePoint::new(I16F16::ZERO, I16F16::from_f64(-6.0), I16F16::ZERO).to_global_fine(),
        FineRotation::IDENTITY,
    )
}

/// One cube at `along_y` metres from the origin, tinted `tint`.
const fn at(along_y: f64, tint: [f32; 4]) -> (Transform, [f32; 4]) {
    (
        Transform::new(
            FinePoint::new(I16F16::ZERO, I16F16::from_f64(along_y), I16F16::ZERO).to_global(),
            Rotation::IDENTITY,
        ),
        tint,
    )
}

/// Records one frame drawing every cube given, and reads it back.
///
/// This is the shape of a `Render::draw`: begin a pass, set a pipeline, bind,
/// draw. Every uniform buffer is made here rather than pooled, because the
/// thing under test is the renderer and a pool would be a second thing to be
/// wrong.
fn drawn(renderer: &mut Renderer, graphics: &Graphics, cubes: &[(Transform, [f32; 4])]) -> Image {
    let projection = matrix::projection(
        Frustum::perspective(
            Angle16::from_degrees(60.0),
            I16F16::from_f64(0.1),
            I16F16::from_f64(100.0),
        ),
        renderer.size().aspect(),
    );
    let camera = watching();
    let view_projection = projection * matrix::view(camera);

    let uniforms: Vec<(wgpu::Buffer, wgpu::BindGroup)> = cubes
        .iter()
        .map(|(transform, tint)| {
            use wgpu::util::DeviceExt as _;
            let clip = view_projection * matrix::model(*transform, camera.position());
            let buffer = renderer
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cube.uniforms"),
                    contents: bytemuck::bytes_of(&Uniforms {
                        clip,
                        tint: *tint,
                        scale: [graphics.mesh.scale, 0.0, 0.0, 0.0],
                    }),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let group = renderer
                .device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("cube.uniforms"),
                    layout: &graphics.layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    }],
                });
            (buffer, group)
        })
        .collect();

    renderer
        .frame(|target: Target<'_>| {
            let depth = graphics
                .depth
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut pass = target
                .encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("opaque"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(NOTHING),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            pass.set_pipeline(&graphics.pipeline);
            for (_, group) in &uniforms {
                pass.set_bind_group(0, group, &[]);
                graphics.mesh.draw(&mut pass, 0..1);
            }
        })
        .unwrap();
    renderer.read_back().unwrap()
}

/// A renderer and the graphics for it, or [`None`] on a machine with no
/// adapter at all.
fn opened(mesh: &Mesh) -> Option<(Renderer, Graphics)> {
    match Renderer::offscreen(SIZE) {
        Ok(renderer) => {
            let graphics = Graphics::new(renderer.device(), renderer.format(), mesh);
            Some((renderer, graphics))
        }
        Err(why) => {
            eprintln!("skipped: this machine has no adapter to render with ({why})");
            None
        }
    }
}

/// The pixel at `x`, `y`, as four bytes.
fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let start = ((y * image.size.width + x) * 4) as usize;
    [
        image.pixels[start],
        image.pixels[start + 1],
        image.pixels[start + 2],
        image.pixels[start + 3],
    ]
}

/// How long one of these is given before the binary calls it wedged.
///
/// A minute, which is far longer than anything else in this workspace waits for
/// anything, and it is the driver rather than the renderer that the margin is
/// for: a whole run of this file takes a second and a half on a machine with a
/// software rasteriser to itself. It is a wide margin because a wedge here is
/// not a slow answer — the observed failure is a device that never answers at
/// all — so anything above the noise does the same job, and the wide one cannot
/// be tripped by a loaded box. What it is *not* is a performance bound. Nothing
/// here asserts anything was fast.
const PATIENCE: Duration = Duration::from_mins(1);

/// How long after [`PATIENCE`] the process is given to die of its own accord
/// before [`impatience`] kills it.
const GRACE: Duration = Duration::from_secs(30);

/// One device at a time.
///
/// Not a fix for anything and not part of any claim here — every test below
/// passes on its own — but several simultaneous Vulkan devices on a software
/// rasteriser is a load nothing in this workspace was ever designed for, and it
/// is the condition under which this file wedges. One at a time is how a window
/// uses a renderer, and it is fast.
static RENDERING: Mutex<()> = Mutex::new(());

/// Kills this process if it is still alive [`GRACE`] after the last test could
/// possibly have finished.
///
/// Armed by the first [`drawing`] and no sooner, so a run that never renders
/// never arms it.
///
/// `abort` and not `exit`, and the difference is the whole reason this exists. A
/// test that gives up on a wedged device leaves the thread where it is, and that
/// thread is inside a graphics driver — so the orderly shutdown `exit` performs
/// runs the driver's own teardown, which waits for the thread that is stuck.
/// The observed result was a binary that printed its failures and then sat at a
/// hundred per cent of a core until somebody found it. `abort` asks nothing of
/// anybody.
fn impatience() {
    static ARMED: OnceLock<()> = OnceLock::new();
    ARMED.get_or_init(|| {
        thread::spawn(|| {
            thread::sleep(PATIENCE + GRACE);
            eprintln!(
                "aborting: this binary was still alive {GRACE:?} after the last test's \
                 deadline, which means a thread abandoned inside the driver is holding the \
                 process open"
            );
            std::process::abort();
        });
    });
}

/// Runs one test's body on a thread of its own, failing the test if it had not
/// finished within [`PATIENCE`].
///
/// The thread is abandoned rather than joined on a timeout, because joining it
/// is the hang being reported: the point is that this process reaches the end of
/// its run and exits with a failure somebody can read.
fn drawing(what: &str, work: impl FnOnce() + Send + 'static) {
    impatience();
    let (finished, done) = mpsc::channel();
    let started = Instant::now();
    thread::spawn(move || {
        // Taken inside the thread rather than outside it, so that waiting for a
        // test that has wedged holding it is under the same deadline as waiting
        // for the device.
        //
        // A poisoned lock is a test that already failed and said so. What is
        // guarded is a graphics device rather than a data structure, and it is
        // no more broken for the last holder having panicked, so the rest of
        // the file still runs.
        let _one_at_a_time = RENDERING.lock().unwrap_or_else(PoisonError::into_inner);
        work();
        // Fails only if this test has already given up and gone, which is the
        // timeout below and is already reported.
        let _ = finished.send(());
    });

    match done.recv_timeout(PATIENCE) {
        Ok(()) => {}
        Err(mpsc::RecvTimeoutError::Timeout) => panic!(
            "{what} had not finished after {PATIENCE:?} (waited {:?}), so the device it is \
             waiting on is not going to answer",
            started.elapsed(),
        ),
        // The sender went without sending, which is the body panicking; its own
        // message is already on the way out.
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("{what} panicked, and its own message is above")
        }
    }
}

#[test]
fn a_cube_covers_the_middle_and_leaves_the_corners_alone() {
    // Both halves are needed. The middle alone would pass on a renderer that
    // filled the frame with the tint and never looked at the geometry, and the
    // corner alone would pass on one that drew nothing.
    drawing(
        "a_cube_covers_the_middle_and_leaves_the_corners_alone",
        || {
            let Some((mut renderer, graphics)) = opened(&cube(None)) else {
                return;
            };
            let image = drawn(&mut renderer, &graphics, &[at(0.0, [0.9, 0.47, 0.16, 1.0])]);

            assert_ne!(pixel(&image, 32, 32), CLEARED, "nothing was drawn");
            assert_eq!(pixel(&image, 0, 0), CLEARED, "the whole frame was drawn");
            // And it is the tint rather than an arbitrary colour: red is the
            // largest channel of the tint and it must be the largest channel on
            // screen, which the clear colour's blue-dominant triple is not.
            let middle = pixel(&image, 32, 32);
            assert!(
                middle[0] > middle[2],
                "the cube came out {middle:?} from an orange tint",
            );
        },
    );
}

#[test]
fn an_empty_frame_leaves_the_clear_colour_everywhere() {
    // The control for the test above: with nothing recorded but the clear,
    // every pixel is what the pass cleared to. A renderer that left the
    // previous frame in place fails here rather than in a test about geometry.
    drawing("an_empty_frame_leaves_the_clear_colour_everywhere", || {
        let Some((mut renderer, graphics)) = opened(&cube(None)) else {
            return;
        };
        let image = drawn(&mut renderer, &graphics, &[]);

        for (index, chunk) in image.pixels.chunks_exact(4).enumerate() {
            assert_eq!(chunk, CLEARED, "pixel {index} is not the clear colour");
        }
    });
}

#[test]
fn the_nearer_cube_hides_the_further_one() {
    // The depth test, in the one direction that distinguishes it from having
    // none: the further cube is drawn *second*, so without depth it would paint
    // over the nearer one. The reverse order is checked too, because a depth
    // test that compared the wrong way would pass the first half alone.
    drawing("the_nearer_cube_hides_the_further_one", || {
        let Some((mut renderer, graphics)) = opened(&cube(None)) else {
            return;
        };
        let near = [0.94, 0.16, 0.16, 1.0];
        let far = [0.16, 0.94, 0.16, 1.0];

        let first = pixel(
            &drawn(&mut renderer, &graphics, &[at(0.0, near), at(30.0, far)]),
            32,
            32,
        );
        let second = pixel(
            &drawn(&mut renderer, &graphics, &[at(30.0, far), at(0.0, near)]),
            32,
            32,
        );

        assert_eq!(first, second, "the order the cubes were listed in mattered");
        assert!(
            first[0] > first[1],
            "the far cube won the depth test: {first:?}",
        );
    });
}

#[test]
fn the_normal_reaches_the_shader_and_is_decoded_there() {
    // The claim the fixed-point vertex adds: two bytes of `Snorm8x2` become a
    // direction in the shader. The same cube is drawn twice with every normal
    // replaced — once pointing at the light and once away from it — so the two
    // frames differ only in that pair of bytes.
    //
    // A shader that ignored the attribute would make the two equal. One that
    // decoded it to a constant would too. Only a decode that reads the pair
    // separates them, and the direction of the difference is asserted as well
    // as its existence: the face turned toward the light is the brighter one,
    // which a decoder with the sign of `w` inverted gets backwards.
    drawing("the_normal_reaches_the_shader_and_is_decoded_there", || {
        // The light in `cube.wgsl` travels along +Y and downwards, so a normal
        // pointing back along -Y catches it and one along +Y does not.
        let toward = OctDirection::encode(Direction::new(
            Signed32::ZERO,
            Signed32::from_f64(-1.0),
            Signed32::ZERO,
        ));
        let away = OctDirection::encode(Direction::new(
            Signed32::ZERO,
            Signed32::from_f64(1.0),
            Signed32::ZERO,
        ));
        assert_ne!(
            toward.to_array(),
            away.to_array(),
            "the fixture is degenerate"
        );

        let white = [1.0, 1.0, 1.0, 1.0];
        let lit = {
            let Some((mut renderer, graphics)) = opened(&cube(Some(toward))) else {
                return;
            };
            pixel(&drawn(&mut renderer, &graphics, &[at(0.0, white)]), 32, 32)
        };
        let unlit = {
            let Some((mut renderer, graphics)) = opened(&cube(Some(away))) else {
                return;
            };
            pixel(&drawn(&mut renderer, &graphics, &[at(0.0, white)]), 32, 32)
        };

        assert!(
            lit[0] > unlit[0],
            "the normal did not change the shading: lit {lit:?}, unlit {unlit:?}",
        );
    });
}

#[test]
fn resizing_changes_what_comes_back() {
    // A resize has to reach the colour texture, and the game's own depth
    // texture has to follow it, or the next frame is a validation error rather
    // than a wrong picture. The cube is still drawn, so a resize that quietly
    // stopped drawing would fail here too.
    drawing("resizing_changes_what_comes_back", || {
        let Some((mut renderer, mut graphics)) = opened(&cube(None)) else {
            return;
        };
        let smaller = Extent::new(32, 16);
        renderer.resize(smaller);
        graphics.depth = depth_texture(renderer.device(), smaller);

        let image = drawn(&mut renderer, &graphics, &[at(0.0, [0.9, 0.47, 0.16, 1.0])]);

        assert_eq!(image.size, smaller);
        assert_eq!(image.pixels.len(), 32 * 16 * 4);
        assert_ne!(pixel(&image, 16, 8), CLEARED, "the resized frame is empty");
    });
}

#[test]
fn the_same_frame_twice_is_the_same_bytes_and_survives_a_png() {
    // Two things at once, and they belong together because each is what makes
    // the other worth anything.
    //
    // The first is what pins the exact-match arm of a capture comparison: one
    // adapter drawing one frame twice produces one answer, so a byte that moved
    // between two runs on the same machine moved for a reason. It says nothing
    // about two *different* adapters, which is the whole reason a PNG golden
    // carries a tolerance.
    //
    // The second is that the encoding is lossless. A capture that quantized or
    // reordered channels on the way to a file would make every later comparison
    // a comparison of the encoder.
    drawing(
        "the_same_frame_twice_is_the_same_bytes_and_survives_a_png",
        || {
            let Some((mut renderer, graphics)) = opened(&cube(None)) else {
                return;
            };
            let orange = [0.9, 0.47, 0.16, 1.0];
            let once = drawn(&mut renderer, &graphics, &[at(0.0, orange)]);
            let twice = drawn(&mut renderer, &graphics, &[at(0.0, orange)]);
            assert_eq!(once.pixels, twice.pixels, "one adapter drew two frames");

            let encoded = once.to_png().unwrap();
            assert_eq!(&encoded[1..4], b"PNG", "that is not a PNG");
            let mut reader = png::Decoder::new(std::io::Cursor::new(&encoded))
                .read_info()
                .unwrap();
            let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
            let info = reader.next_frame(&mut pixels).unwrap();
            pixels.truncate(info.buffer_size());
            assert_eq!(pixels, once.pixels, "the PNG is not the frame");
        },
    );
}
