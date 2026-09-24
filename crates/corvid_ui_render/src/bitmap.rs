//! A face that ships in the binary: eight by eight pixels a glyph, public
//! domain, and enough of Unicode for a menu, a heads-up display and a box
//! drawn out of line characters.
//!
//! The seam against `atlas.rs` is that this is a *font* as well as an atlas:
//! it answers [`Metrics`] for the layout and [`Atlas`] for the device from one
//! table, so a label is measured against the same cells it is drawn from.

use alloc::vec::Vec;

use corvid_fixed::I16F16;
use corvid_ui::{GlyphId, Metrics};
use font8x8::legacy::{BASIC_LEGACY, BLOCK_LEGACY, BOX_LEGACY, LATIN_LEGACY};

use crate::{Atlas, Grid};

/// The first code point of the box-drawing block, which starts the second
/// half of the atlas.
const BOX: u32 = 0x2500;

/// The block-element code points that follow the box-drawing ones.
const BLOCK: u32 = 0x2580;

/// Where the non-breaking space starts the Latin-1 supplement.
const LATIN: u32 = 0xA0;

/// The cell a character this face does not draw is shown as.
const MISSING: u32 = b'?' as u32;

/// The eight-by-eight bitmap face, as a layout font and as an atlas at once.
///
/// ASCII, the Latin-1 supplement, the box-drawing block and the block
/// elements: four hundred and sixteen cells in a thirty-two column grid, one
/// byte of coverage a pixel. A character outside those draws as `?` rather
/// than as nothing, because a missing glyph that takes no space is a label
/// that silently reads differently from what the program wrote.
///
/// ```
/// use corvid_fixed::I16F16;
/// use corvid_ui::Metrics as _;
/// use corvid_ui_render::Font8x8;
///
/// let face = Font8x8;
/// let size = I16F16::from_f64(16.0);
/// // A cell is square, so a glyph is as wide as the size it is set at.
/// assert_eq!(face.advance(face.glyph('A'), size), size);
/// // A line character lands in the second half of the atlas.
/// assert!(face.glyph('\u{2502}').0 >= 256);
/// ```
///
/// Sample it with a nearest filter: a bitmap face scaled with a linear one is
/// a blur, and the whole point of eight pixels is that they stay pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Font8x8;

impl Font8x8 {
    /// Cells across the atlas.
    pub const COLUMNS: u32 = 32;

    /// Cells down the atlas.
    pub const ROWS: u32 = 13;

    /// Pixels a cell is on each side.
    pub const CELL: u32 = 8;

    /// The grid the cells are laid out on, starting at code point zero.
    const GRID: Grid = Grid::new(Self::COLUMNS, Self::ROWS, 0);

    /// Which cell a character is drawn from, or [`None`] if this face does
    /// not have it.
    #[must_use]
    pub fn cell(character: char) -> Option<u32> {
        let point = u32::from(character);
        match point {
            0..0x80 | LATIN..0x100 => Some(point),
            BOX..BLOCK => Some(256 + point - BOX),
            BLOCK..0x25A0 => Some(256 + 128 + point - BLOCK),
            _ => None,
        }
    }

    /// The eight rows of one cell, least significant bit leftmost, which is
    /// how the source tables store them.
    fn rows(cell: u32) -> [u8; 8] {
        let index = |at: u32| usize::try_from(at).unwrap_or(usize::MAX);
        match cell {
            0..0x80 => BASIC_LEGACY.get(index(cell)),
            LATIN..0x100 => LATIN_LEGACY.get(index(cell - LATIN)),
            256..384 => BOX_LEGACY.get(index(cell - 256)),
            384..416 => BLOCK_LEGACY.get(index(cell - 384)),
            _ => None,
        }
        .copied()
        .unwrap_or([0; 8])
    }

    /// The atlas's width and height in pixels.
    #[must_use]
    pub const fn extent() -> (u32, u32) {
        (Self::COLUMNS * Self::CELL, Self::ROWS * Self::CELL)
    }

    /// The whole atlas as one byte of coverage a pixel, row-major, `0xFF`
    /// where there is ink.
    ///
    /// What an `R8Unorm` texture is uploaded from; [`upload`](Self::upload)
    /// is that upload.
    #[must_use]
    pub fn coverage() -> Vec<u8> {
        let (width, height) = Self::extent();
        let mut pixels = alloc::vec![0_u8; (width * height) as usize];
        for cell in 0..Self::COLUMNS * Self::ROWS {
            let (left, top) = (
                (cell % Self::COLUMNS) * Self::CELL,
                (cell / Self::COLUMNS) * Self::CELL,
            );
            for (y, row) in (0_u32..).zip(Self::rows(cell)) {
                for x in 0..Self::CELL {
                    if row & (1 << x) != 0 {
                        let at = (top + y) * width + left + x;
                        if let Some(pixel) = pixels.get_mut(at as usize) {
                            *pixel = 0xFF;
                        }
                    }
                }
            }
        }
        pixels
    }

    /// The atlas on a device, and the nearest-filtered sampler it is read
    /// through: the two things [`Painter::new`](crate::Painter::new) asks for.
    #[must_use]
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> (wgpu::TextureView, wgpu::Sampler) {
        let (width, height) = Self::extent();
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("corvid_ui.font8x8"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &Self::coverage(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width),
                rows_per_image: Some(height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("corvid_ui.font8x8"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..wgpu::SamplerDescriptor::default()
        });
        (view, sampler)
    }
}

impl Metrics for Font8x8 {
    fn glyph(&self, character: char) -> GlyphId {
        GlyphId(Self::cell(character).unwrap_or(MISSING))
    }

    /// A cell is square, so the advance is the size.
    fn advance(&self, _glyph: GlyphId, size: I16F16) -> I16F16 {
        size
    }

    /// A quarter of a cell between lines, which is what keeps two rows of a
    /// table from reading as one block.
    fn line_height(&self, size: I16F16) -> I16F16 {
        size.saturating_add(size.saturating_mul(I16F16::from_f64(0.25)))
    }

    /// The baseline one cell below the top of the line box, so the cell's
    /// bottom row -- where the descenders are -- sits under it.
    fn ascent(&self, size: I16F16) -> I16F16 {
        size
    }
}

impl Atlas for Font8x8 {
    fn uv(&self, glyph: GlyphId) -> [f32; 4] {
        Self::GRID.uv(glyph)
    }

    /// The cell, with seven of its eight rows above the baseline.
    fn quad(&self, _glyph: GlyphId) -> [f32; 4] {
        [0.0, -0.875, 1.0, 1.0]
    }
}
