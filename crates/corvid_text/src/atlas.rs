//! One page, many glyphs.

use crate::{AtlasFull, Coverage};
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use corvid_fixed::I16F16;
use corvid_float::demote;
use corvid_ui::GlyphId;

/// A gap of one pixel around every glyph.
///
/// A sampler asked for a texel on the edge of a glyph reads its neighbours, so
/// two glyphs packed flush would bleed into each other under any filtering at
/// all. One pixel is enough for bilinear, which is what a UI pass uses; a
/// mipmapped atlas would want more, and this crate does not build one.
const PAD: u32 = 1;

/// Where one glyph landed on the page.
///
/// The four pixel coordinates are the rectangle in the texture; `left` and
/// `top` are where it goes relative to the pen, and are the same numbers the
/// [`Coverage`] carried.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Slot {
    /// Left edge on the page, in pixels.
    pub x: u32,
    /// Top edge on the page, in pixels.
    pub y: u32,
    /// Width on the page, in pixels.
    pub width: u32,
    /// Height on the page, in pixels.
    pub height: u32,
    /// Where its left edge sits, right of the pen, in pixels.
    pub left: i32,
    /// Where its top edge sits, down from the baseline, in pixels. Normally
    /// negative.
    pub top: i32,
}

/// A page of glyphs at one size, packed onto shelves.
///
/// The packer is a shelf packer and nothing cleverer: glyphs go left to right
/// on a row as tall as the tallest one on it, and a row that runs out of width
/// starts a new one below. For a face at a size -- which is what an atlas is --
/// the glyphs are all within a couple of pixels of each other's height, and the
/// waste a shelf packer leaves is the difference between a capital and a comma.
/// A packer that beat it would need to see every glyph up front, and a game
/// that discovers a new accent when a new line of dialogue arrives cannot.
///
/// The page is coverage, one byte to the pixel, and it is deliberately not a
/// texture: uploading one is the device ring's job.
///
/// ```
/// use corvid_fixed::I16F16;
/// use corvid_text::{Atlas, Coverage};
/// use corvid_ui::GlyphId;
///
/// let mut atlas = Atlas::new(8, 8, I16F16::from_f64(16.0));
/// let Some(block) = Coverage::new(2, 2, 0, -2, vec![0xff; 4]) else { return };
/// let Ok(slot) = atlas.insert(GlyphId(1), &block) else { return };
/// // One pixel in from the corner, because every glyph is padded.
/// assert_eq!((slot.x, slot.y, slot.width, slot.height), (1, 1, 2, 2));
/// assert_eq!(atlas.slot(GlyphId(1)), Some(slot), "and it is remembered");
/// assert_eq!(atlas.slot(GlyphId(2)), None, "and nothing else is");
/// // Two pixels of a sixteen-pixel em, and two of an eight-pixel page.
/// assert_eq!(atlas.quad(GlyphId(1)), [0.0, -0.125, 0.125, 0.125]);
/// assert_eq!(atlas.uv(GlyphId(1)), [0.125, 0.125, 0.375, 0.375]);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Atlas {
    /// Page width in pixels.
    width: u32,
    /// Page height in pixels.
    height: u32,
    /// The size the glyphs on it were rasterised at, so that [`Atlas::quad`]
    /// can answer in multiples of the em rather than in pixels.
    size: I16F16,
    /// The coverage, row-major, `width * height` of it.
    pixels: Vec<u8>,
    /// Where each glyph landed. Ordered, so that two atlases filled with the
    /// same glyphs iterate the same way.
    slots: BTreeMap<GlyphId, Slot>,
    /// The left edge of the next glyph on the current shelf.
    pen: u32,
    /// The top edge of the current shelf.
    shelf: u32,
    /// How tall the current shelf is, which is its tallest glyph.
    tall: u32,
}

impl Atlas {
    /// An empty page, `width` by `height` pixels, for glyphs rasterised at
    /// `size`.
    #[must_use]
    pub fn new(width: u32, height: u32, size: I16F16) -> Self {
        let area = usize::try_from(width)
            .unwrap_or(usize::MAX)
            .saturating_mul(usize::try_from(height).unwrap_or(usize::MAX));
        Self {
            width,
            height,
            size,
            pixels: vec![0; area],
            slots: BTreeMap::new(),
            pen: PAD,
            shelf: PAD,
            tall: 0,
        }
    }

    /// Put `coverage` on the page under `glyph`.
    ///
    /// A glyph already on the page keeps the slot it has and the pixels are not
    /// written again, so inserting the same glyph twice is cheap and does not
    /// move anything a caller has already drawn with.
    ///
    /// # Errors
    ///
    /// [`AtlasFull`] when the page has no room left. Nothing is written and no
    /// slot is recorded, so an atlas that reports full is still a correct atlas
    /// of everything that fitted before it.
    pub fn insert(&mut self, glyph: GlyphId, coverage: &Coverage) -> Result<Slot, AtlasFull> {
        if let Some(slot) = self.slots.get(&glyph) {
            return Ok(*slot);
        }
        let (width, height) = (coverage.width(), coverage.height());
        let slot = if coverage.is_blank() {
            // A space has no pixels, and a page with no room still has room for
            // no pixels. Recording it keeps `slot` total over the glyphs a run
            // asks about, which is what stops a caller special-casing the space.
            Slot {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
                left: coverage.left(),
                top: coverage.top(),
            }
        } else {
            let (x, y) = self.reserve(width, height).ok_or(AtlasFull {
                glyph,
                width,
                height,
                page_width: self.width,
                page_height: self.height,
            })?;
            self.blit(x, y, coverage);
            Slot {
                x,
                y,
                width,
                height,
                left: coverage.left(),
                top: coverage.top(),
            }
        };
        self.slots.insert(glyph, slot);
        Ok(slot)
    }

    /// Where the next glyph of this size goes, or `None` when the page is full.
    fn reserve(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width > self.width.saturating_sub(PAD * 2) {
            return None;
        }
        if self.pen.saturating_add(width).saturating_add(PAD) > self.width {
            self.shelf = self.shelf.saturating_add(self.tall).saturating_add(PAD);
            self.pen = PAD;
            self.tall = 0;
        }
        if self.shelf.saturating_add(height).saturating_add(PAD) > self.height {
            return None;
        }
        let placed = (self.pen, self.shelf);
        self.pen = self.pen.saturating_add(width).saturating_add(PAD);
        self.tall = self.tall.max(height);
        Some(placed)
    }

    /// Copy the coverage onto the page at `x`, `y`.
    fn blit(&mut self, x: u32, y: u32, coverage: &Coverage) {
        let width = usize::try_from(self.width).unwrap_or(usize::MAX);
        for row in 0..coverage.height() {
            for column in 0..coverage.width() {
                let target = usize::try_from(y.saturating_add(row))
                    .ok()
                    .and_then(|row| row.checked_mul(width))
                    .and_then(|offset| {
                        offset.checked_add(usize::try_from(x.saturating_add(column)).ok()?)
                    });
                if let Some(cell) = target.and_then(|index| self.pixels.get_mut(index)) {
                    *cell = coverage.at(column, row);
                }
            }
        }
    }

    /// Where `glyph` is, or `None` when it has not been inserted.
    #[must_use]
    pub fn slot(&self, glyph: GlyphId) -> Option<Slot> {
        self.slots.get(&glyph).copied()
    }

    /// How many glyphs are on the page.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether nothing has been inserted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Page width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Page height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The size the glyphs on it were rasterised at.
    #[must_use]
    pub const fn size(&self) -> I16F16 {
        self.size
    }

    /// The page, row-major, one byte of coverage to the pixel.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// This glyph's corners on the page, as `[u0, v0, u1, v1]` in `0..=1`.
    ///
    /// A zero-area rectangle for a glyph the page does not hold and for one
    /// with no ink, which samples nothing and draws nothing. These are the two
    /// functions `corvid_ui_render::Atlas` asks for, in the order and the units
    /// it asks for them, so a renderer implements that trait over this in two
    /// forwarding lines.
    #[must_use]
    pub fn uv(&self, glyph: GlyphId) -> [f32; 4] {
        let Some(slot) = self.slot(glyph) else {
            return [0.0; 4];
        };
        let (across, down) = (f64::from(self.width.max(1)), f64::from(self.height.max(1)));
        [
            demote(f64::from(slot.x) / across),
            demote(f64::from(slot.y) / down),
            demote(f64::from(slot.x.saturating_add(slot.width)) / across),
            demote(f64::from(slot.y.saturating_add(slot.height)) / down),
        ]
    }

    /// How large this glyph is on the line, as a multiple of the em size:
    /// `[left, top, width, height]`, right and down from the pen on the
    /// baseline.
    ///
    /// `top` is normally negative, because a glyph sits above the line it is on.
    #[must_use]
    pub fn quad(&self, glyph: GlyphId) -> [f32; 4] {
        let Some(slot) = self.slot(glyph) else {
            return [0.0; 4];
        };
        let em = self.size.to_f64();
        if em <= 0.0 {
            return [0.0; 4];
        }
        [
            demote(f64::from(slot.left) / em),
            demote(f64::from(slot.top) / em),
            demote(f64::from(slot.width) / em),
            demote(f64::from(slot.height) / em),
        ]
    }
}
