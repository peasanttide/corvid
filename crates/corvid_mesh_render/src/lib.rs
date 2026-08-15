#![doc = include_str!("../README.md")]

// No `no_std`. A buffer is created on a device, and this is the layer whose job
// that is; `corvid_mesh` is the half that has no device in it.

use corvid_mesh::{Mesh, Vertex};

/// The layout to name in a pipeline's `buffers`, so that a game's shader
/// reads `@location(0) position: vec4<f32>` and `@location(1) normal:
/// vec2<f32>` without unpacking anything.
///
/// Both attributes arrive already normalized: the position in `[-1, 1]` per
/// axis, and the normal as the two octahedral components `OctDirection`
/// stores. Decoding the second is four lines of WGSL and `examples/hello` has
/// them.
///
/// A free `const` rather than `Vertex::LAYOUT`, because [`Vertex`] lives in
/// `corvid_mesh` and the orphan rule does not let this crate add an inherent
/// item to it. That is the price of `corvid_mesh` being usable by a project
/// that compiles no graphics stack at all.
pub const VERTEX_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &wgpu::vertex_attr_array![0 => Snorm16x4, 1 => Snorm8x2],
};

/// Uploads the two buffers a draw call needs.
///
/// `label` is what the two buffers are named on the device, with
/// `.vertices` and `.indices` appended -- so a capture in a graphics
/// debugger says which mesh it is looking at rather than "Buffer 41".
///
/// The indices are `Uint32`, which is what [`Uploaded::draw`] sets, and there
/// is no `Uint16` path: a mesh small enough for sixteen-bit indices saves two
/// bytes an index against the twelve a vertex already costs, and having one
/// index width is worth more than that.
#[must_use]
pub fn upload(mesh: &Mesh, device: &wgpu::Device, label: &str) -> Uploaded {
    use wgpu::util::DeviceExt as _;

    let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("{label}.vertices")),
        contents: bytemuck::cast_slice(&mesh.vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("{label}.indices")),
        contents: bytemuck::cast_slice(&mesh.indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    Uploaded {
        vertices,
        indices,
        count: u32::try_from(mesh.indices.len()).unwrap_or(u32::MAX),
        scale: mesh.scale.to_f32(),
    }
}

/// [`Mesh::scale`] as the `f32` a shader multiplies by.
///
/// A mesh that lives on the device.
///
/// The buffers are public because a game's pipeline binds them itself; what
/// this adds over two loose `wgpu::Buffer`s is the index count, which is the
/// number a draw call needs and the one thing nothing else remembers.
#[derive(Debug)]
pub struct Uploaded {
    /// The vertices, laid out as [`VERTEX_LAYOUT`] says.
    pub vertices: wgpu::Buffer,
    /// The indices, `Uint32`.
    pub indices: wgpu::Buffer,
    /// How many indices there are, which is three per triangle.
    pub count: u32,
    /// [`Mesh::scale`] as the `f32` a shader reads.
    ///
    /// Converted here rather than on every frame, and it is the only `f32` in
    /// this crate: a mesh is fixed-point data right up to the moment it is
    /// handed to a device.
    pub scale: f32,
}

impl Uploaded {
    /// Binds this mesh into `pass` and draws `instances` copies of it.
    ///
    /// The vertex buffer goes to slot zero, which is what [`VERTEX_LAYOUT`]
    /// being the first entry of a pipeline's `buffers` means. A game with
    /// per-instance data puts it in slot one and sets it itself.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, instances: core::ops::Range<u32>) {
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.count, 0, instances);
    }
}
