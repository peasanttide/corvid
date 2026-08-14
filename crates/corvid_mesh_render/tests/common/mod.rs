//! A device, a cube and a frame read back off it.
//!
//! The seam against the tests that use it is assertion: nothing here compares
//! anything. It opens an adapter if there is one, draws what it is given, and
//! hands back pixels.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]
#![allow(
    dead_code,
    reason = "each integration test binary compiles this module separately, so anything only one of them uses is dead in the others"
)]
#![allow(
    clippy::print_stderr,
    reason = "a test that is skipped has to say so where a person running the suite will see it, and the workspace's answer everywhere else -- a tracing event -- needs a subscriber that a test harness does not install"
)]

use std::{sync::Mutex, time::Duration};

use corvid_camera::matrix;
use corvid_fixed::{Angle16, I16F16, Signed32};
use corvid_glm::Mat4;
use corvid_mesh::{Mesh, Vertex};
use corvid_mesh_render::{Uploaded, VERTEX_LAYOUT, upload};
use corvid_render::Target;
pub(crate) use corvid_render::{Extent, Image, Renderer};
use corvid_rotation::{FineRotation, Rotation};
use corvid_shape::Frustum;
use corvid_transform::FineTransform;
pub(crate) use corvid_transform::Transform;
use corvid_vector::FinePoint;
pub(crate) use corvid_vector::{Direction, OctDirection};

/// How big the frames are. Small, because every pixel is read back and
/// compared and nothing here is about resolution.
pub(crate) const SIZE: Extent = Extent::new(64, 64);

/// What the pass clears to: a dark blue nothing else in these tests is.
pub(crate) const NOTHING: wgpu::Color = wgpu::Color {
    r: 8.0 / 255.0,
    g: 12.0 / 255.0,
    b: 40.0 / 255.0,
    a: 1.0,
};

/// The same colour as the eight-bit bytes it reads back as.
pub(crate) const CLEARED: [u8; 4] = [8, 12, 40, 255];

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
pub(crate) fn cube(facing: Option<OctDirection>) -> Mesh {
    /// Each face as its outward normal, a tangent and a bitangent, in that
    /// order, with `tangent x bitangent = normal`.
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
pub(crate) struct Graphics {
    pub(crate) pipeline: wgpu::RenderPipeline,
    pub(crate) layout: wgpu::BindGroupLayout,
    pub(crate) mesh: Uploaded,
    pub(crate) depth: wgpu::Texture,
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
pub(crate) fn depth_texture(device: &wgpu::Device, size: Extent) -> wgpu::Texture {
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
const fn watching() -> FineTransform {
    FineTransform::new(
        FinePoint::new(I16F16::ZERO, I16F16::from_f64(-6.0), I16F16::ZERO).to_global_fine(),
        FineRotation::IDENTITY,
    )
}

/// One cube at `along_y` metres from the origin, tinted `tint`.
pub(crate) const fn at(along_y: f64, tint: [f32; 4]) -> (Transform, [f32; 4]) {
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
pub(crate) fn drawn(
    renderer: &mut Renderer,
    graphics: &Graphics,
    cubes: &[(Transform, [f32; 4])],
) -> Image {
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
        .frame(|target: Target<'_>, encoder: &mut wgpu::CommandEncoder| {
            let depth = graphics
                .depth
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
pub(crate) fn opened(mesh: &Mesh) -> Option<(Renderer, Graphics)> {
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
pub(crate) fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
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
/// not a slow answer -- the observed failure is a device that never answers at
/// all -- so anything above the noise does the same job, and the wide one cannot
/// be tripped by a loaded box. What it is *not* is a performance bound. Nothing
/// here asserts anything was fast.
pub(crate) const PATIENCE: Duration = Duration::from_mins(1);

/// How long after [`PATIENCE`] the process is given to die of its own accord
/// before [`impatience`] kills it.
pub(crate) const GRACE: Duration = Duration::from_secs(30);

/// One device at a time.
///
/// Not a fix for anything and not part of any claim here -- every test below
/// passes on its own -- but several simultaneous Vulkan devices on a software
/// rasteriser is a load nothing in this workspace was ever designed for, and it
/// is the condition under which this file wedges. One at a time is how a window
/// uses a renderer, and it is fast.
pub(crate) static RENDERING: Mutex<()> = Mutex::new(());

pub(crate) mod deadline;
