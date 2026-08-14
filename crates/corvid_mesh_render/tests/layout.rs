//! The layout the device reads, frozen against the bytes a vertex is.
//!
//! `corvid_mesh/tests/vertex.rs` pins the twelve bytes; this pins the offsets
//! and formats a pipeline is told to read them at. The two have to agree and
//! nothing in the type system says so: a field reordered in Rust and not here
//! is a mesh that renders as noise, and neither half alone would notice.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_mesh::Vertex;
use corvid_mesh_render::VERTEX_LAYOUT;
use corvid_render::wgpu;
#[test]
fn the_layout_the_device_reads_addresses_those_bytes() {
    assert_eq!(VERTEX_LAYOUT.array_stride, 12);
    assert_eq!(
        VERTEX_LAYOUT.array_stride,
        size_of::<Vertex>() as wgpu::BufferAddress,
    );
    assert_eq!(VERTEX_LAYOUT.step_mode, wgpu::VertexStepMode::Vertex);

    let attributes: Vec<(u32, u64, wgpu::VertexFormat)> = VERTEX_LAYOUT
        .attributes
        .iter()
        .map(|attribute| {
            (
                attribute.shader_location,
                attribute.offset,
                attribute.format,
            )
        })
        .collect();
    assert_eq!(
        attributes,
        vec![
            (0, 0, wgpu::VertexFormat::Snorm16x4),
            (1, 8, wgpu::VertexFormat::Snorm8x2),
        ],
    );

    // And the two attributes together do not run off the end of a vertex,
    // which is the validation error the pair above would otherwise become.
    let last = attributes.last().unwrap();
    assert!(last.1 + last.2.size() <= VERTEX_LAYOUT.array_stride);
}
