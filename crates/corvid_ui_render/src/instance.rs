//! The two instance layouts, and the paint data turned into them.
//!
//! The seam against `painter.rs` is that nothing here touches a device: these
//! are `#[repr(C)]` values a vertex buffer is made of, and they are built from
//! a `corvid_ui` layout on the CPU.

use corvid_ui::{PaintedGlyph, PaintedRect, Rect};

use crate::Atlas;

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
