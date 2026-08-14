//! Where a glyph is in the atlas texture.
//!
//! The seam is that rasterising a font is a different crate's job: what is
//! here is only how to find the result once somebody else has put it in a
//! texture.

use corvid_float::demote as narrow;
use corvid_ui::GlyphId;

/// Where each glyph is in the atlas texture.
///
/// A trait rather than a texture, because rasterising a font is a different
/// crate's job and this one only needs to know where the result landed.
///
/// [`Grid`] is the one this crate ships. A shaper that lays glyphs out its own
/// way implements this instead, and owes two functions:
///
/// ```
/// use corvid_ui::GlyphId;
/// use corvid_ui_render::Atlas;
///
/// /// One glyph, filling the whole page: the least atlas that is still one.
/// struct Only;
///
/// impl Atlas for Only {
///     fn uv(&self, glyph: GlyphId) -> [f32; 4] {
///         if u32::from(glyph) == 0 { [0.0, 0.0, 1.0, 1.0] } else { [0.0; 4] }
///     }
///
///     fn quad(&self, _glyph: GlyphId) -> [f32; 4] {
///         [0.0, -1.0, 1.0, 1.0]
///     }
/// }
///
/// assert_eq!(Only.uv(GlyphId(0)), [0.0, 0.0, 1.0, 1.0]);
/// assert_eq!(Only.uv(GlyphId(1)), [0.0; 4], "a glyph it does not hold draws nothing");
/// ```
pub trait Atlas {
    /// This glyph's corners in the atlas, as `[u0, v0, u1, v1]` in `0..=1`.
    ///
    /// A glyph the atlas does not hold answers a zero-area rectangle, which
    /// samples nothing and draws nothing.
    fn uv(&self, glyph: GlyphId) -> [f32; 4];

    /// How large this glyph is on the page, as a multiple of the em size.
    ///
    /// `[left, top, width, height]`, right and down from the pen position on
    /// the baseline -- so `top` is normally negative, because a glyph sits
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
/// use corvid_ui::GlyphId;
/// use corvid_ui_render::{Atlas as _, Grid};
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
        // above the baseline -- the proportions `corvid_ui::Monospace` measures
        // with, so a layout and its glyphs agree without a second table.
        [0.0, -0.75, 1.0, 1.0]
    }
}
