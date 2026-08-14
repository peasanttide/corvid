//! How a painted layout splits into scissored runs.
//!
//! The seam is the scissor: a subtree is contiguous in tree order and a clip
//! region is a subtree, so this is the one part that has to walk both instance
//! lists at once, and it does so with no device in sight.

use alloc::vec::Vec;

use corvid_ui::Painted;

/// One run of instances under one scissor rectangle.
///
/// A UI with one scroll region is two batches; a UI with fifty is fifty, and
/// that is the number to watch if a HUD ever gets slow.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Batch {
    /// Which entry of [`Painted::clips`] this run is scissored to.
    pub clip: u32,
    /// The rectangles in it.
    pub rects: core::ops::Range<u32>,
    /// The glyphs in it.
    pub glyphs: core::ops::Range<u32>,
}

/// How a painted layout splits into scissored runs.
///
/// A subtree is contiguous in tree order and a clip region is a subtree, so a
/// run of one clip index is a run in both lists at once. Within one run the
/// rectangles are drawn and then the glyphs, which is what puts a label over
/// the panel it is on.
///
/// ```
/// use corvid_ui::Painted;
/// use corvid_ui_render::batches;
///
/// // Nothing to draw is no batches, and therefore no draw calls.
/// assert!(batches(&Painted::default()).is_empty());
/// ```
#[must_use]
pub fn batches(painted: &Painted) -> Vec<Batch> {
    let mut out = Vec::new();
    let (mut rect, mut glyph) = (0, 0);
    while rect < painted.rects.len() || glyph < painted.glyphs.len() {
        let clip = painted.rects.get(rect).map_or_else(
            || painted.glyphs.get(glyph).map_or(0, |it| it.clip),
            |it| it.clip,
        );
        let first_rect = rect;
        while painted.rects.get(rect).is_some_and(|it| it.clip == clip) {
            rect += 1;
        }
        let first_glyph = glyph;
        while painted.glyphs.get(glyph).is_some_and(|it| it.clip == clip) {
            glyph += 1;
        }
        out.push(Batch {
            clip,
            rects: count(first_rect)..count(rect),
            glyphs: count(first_glyph)..count(glyph),
        });
    }
    out
}

/// An index as the `u32` a draw call takes.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a draw call's instance range is a u32, so a UI with four billion rectangles could not be drawn whatever this returned"
)]
pub(crate) const fn count(index: usize) -> u32 {
    index as u32
}
