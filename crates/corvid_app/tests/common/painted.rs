//! The drawing half, for the runs that open a device.
//!
//! The seam against `client.rs` is the adapter: nothing there needs one, and
//! nothing here can be built without one.

use corvid_behavior::{Extract, Extracting};
use corvid_render::wgpu;

use super::Tally;

/// The drawing half of [`Tally`], for the runs that open a device.
///
/// One triangle in clip space with no camera, no uniform and no depth: the
/// claim `tests/windowless.rs` makes is about a *digest*, so what this has to
/// be is drawable -- a mesh a device actually uploads and rasterises, so that
/// the run being compared is a run in which a device did work.
///
/// This is where the view and the pipelines are declared, because `Render` is
/// the base of the client-local half: `Present` reads and writes the view in
/// all three of its functions and declares neither.
impl Extract<Tally> for Painted {
    fn extract(&mut self, _extracting: Extracting<'_, Tally>) {}
}

impl corvid_render::Render<Tally> for Painted {
    type Config = ();

    fn new(opened: corvid_render::Opened<'_>, (): ()) -> Self {
        Self::setup(opened.device, opened.queue, opened.format)
    }

    fn configure(&mut self, (): ()) {}

    fn draw(
        &mut self,
        drawing: corvid_render::Drawing<'_>,
        encoder: &mut corvid_render::wgpu::CommandEncoder,
    ) {
        use corvid_render::wgpu;

        let target = drawing.target;
        let graphics = self;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tally"),
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
        pass.set_pipeline(&graphics.pipeline);
        graphics.triangle.draw(&mut pass, 0..1);
    }
}

/// Built once, where the device is.
impl Painted {
    fn setup(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        use corvid_render::wgpu;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tally"),
            source: wgpu::ShaderSource::Wgsl(include_str!("tally.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tally"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        Self {
            pipeline: device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("tally"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(corvid_mesh_render::VERTEX_LAYOUT)],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fragment_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(format.into())],
                }),
                multiview_mask: None,
                cache: None,
            }),
            triangle: corvid_mesh_render::upload(&triangle(), device, "tally.triangle"),
        }
    }
}

/// What [`Tally`]'s renderer builds once.
#[derive(Debug)]
pub(crate) struct Painted {
    /// The one pipeline.
    pipeline: wgpu::RenderPipeline,
    /// The one mesh.
    triangle: corvid_mesh_render::Uploaded,
}

/// One triangle, which is the least geometry that is still geometry.
fn triangle() -> corvid_mesh::Mesh {
    use corvid_mesh::{Mesh, Vertex};
    use corvid_vector::OctDirection;
    Mesh::new(
        vec![
            Vertex::new([-Vertex::FULL, -Vertex::FULL, 0], OctDirection::UP),
            Vertex::new([Vertex::FULL, -Vertex::FULL, 0], OctDirection::UP),
            Vertex::new([0, Vertex::FULL, 0], OctDirection::UP),
        ],
        vec![0, 1, 2],
        corvid_fixed::I16F16::from_f64(1.0),
    )
}
