//! A string, turned into glyphs that are somewhere.

use alloc::vec::Vec;
use corvid_fixed::I16F16;
use corvid_ui::{GlyphId, Metrics};

/// What a shaper needs from a face past what a layout needs.
///
/// [`Metrics`] answers the three numbers a box solver wants and has to answer
/// something for every character. Shaping wants two more things: whether the
/// face actually had the character, and how a pair of glyphs move together.
/// Both are defaulted, so a face that neither kerns nor has holes -- a fixed
/// grid, a bitmap font -- implements this with an empty block.
///
/// ```
/// use corvid_fixed::I16F16;
/// use corvid_text::{NOTDEF, Shaping};
/// use corvid_ui::{GlyphId, Monospace};
///
/// // A face whose glyph number is the character, and which has the letters and
/// // nothing else -- so an accent is a hole rather than a letter.
/// struct Ascii;
///
/// impl Shaping for Ascii {
///     fn lookup(&self, character: char) -> Option<GlyphId> {
///         character.is_ascii().then(|| GlyphId(character as u32))
///     }
/// }
/// # impl corvid_ui::Metrics for Ascii {
/// #     fn glyph(&self, c: char) -> GlyphId { self.lookup(c).unwrap_or(NOTDEF) }
/// #     fn advance(&self, _: GlyphId, size: I16F16) -> I16F16 { size }
/// #     fn line_height(&self, size: I16F16) -> I16F16 { size }
/// #     fn ascent(&self, size: I16F16) -> I16F16 { size }
/// # }
/// assert_eq!(Ascii.lookup('e'), Some(GlyphId(0x65)));
/// assert_eq!(Ascii.lookup('\u{e9}'), None);
/// // Monospace has no holes and no kerning, so its block is empty.
/// assert_eq!(Monospace::DEFAULT.lookup('\u{e9}'), Some(GlyphId(0xe9)));
/// assert_eq!(
///     Monospace::DEFAULT.kern(GlyphId(b'A'.into()), GlyphId(b'V'.into()), I16F16::ONE),
///     I16F16::ZERO,
/// );
/// ```
pub trait Shaping: Metrics {
    /// The glyph for `character`, or `None` when the face has none.
    ///
    /// The default says every character resolves, which is true of a face whose
    /// glyph number *is* the character.
    fn lookup(&self, character: char) -> Option<GlyphId> {
        Some(self.glyph(character))
    }

    /// The extra advance between `left` and `right` at `size`, negative where
    /// the pair tucks together.
    ///
    /// The default is no kerning at all, which is the right answer for a face
    /// whose glyphs are all one width.
    fn kern(&self, left: GlyphId, right: GlyphId, size: I16F16) -> I16F16 {
        let _ = (left, right, size);
        I16F16::ZERO
    }
}

impl Shaping for corvid_ui::Monospace {}

/// The glyph a face draws for a character it does not have.
///
/// Glyph zero, which every TrueType face is required to define and which is
/// conventionally an empty box. [`shape`] places it rather than dropping the
/// character, so that a missing accent is a hole in the word on the screen
/// instead of a word that silently lost a letter.
pub const NOTDEF: GlyphId = GlyphId(0);

/// One glyph, and where it goes.
///
/// The position is fixed point because it feeds a layout: a text box's width
/// decides where the button beside it lands, and a box that measured a
/// fractionally different width on two machines would put the button in two
/// places. Coverage is float and this is not, and the seam between them is
/// exactly here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PositionedGlyph {
    /// Which glyph to draw.
    pub glyph: GlyphId,
    /// Where its pen sits, right of the start of the run.
    pub x: I16F16,
    /// Where its baseline sits, down from the top of the paragraph. Zero for a
    /// run shaped on its own.
    pub y: I16F16,
    /// The byte offset in the source string of the character that asked for it.
    pub cluster: u32,
    /// The character that asked for it.
    pub character: char,
    /// Whether the face had no glyph for that character, so this is [`NOTDEF`].
    pub missing: bool,
}

/// A string of text, shaped: the glyphs, in order, each with a pen position.
///
/// One line and one direction. A newline in the text is shaped like any other
/// character the face may or may not have, because deciding where lines end is
/// [`wrap`](crate::wrap)'s job rather than this one's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Run {
    /// The glyphs, in the order the characters came in.
    glyphs: Vec<PositionedGlyph>,
    /// Where the pen ended up, which is the width of the run.
    width: I16F16,
}

impl Run {
    /// The glyphs, in source order.
    #[must_use]
    pub fn glyphs(&self) -> &[PositionedGlyph] {
        &self.glyphs
    }

    /// How wide the run is: the pen's position after the last glyph, kerning
    /// included.
    #[must_use]
    pub const fn width(&self) -> I16F16 {
        self.width
    }

    /// Whether nothing was placed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// The glyphs the face did not have, in source order.
    ///
    /// Each one is drawn, as the empty box glyph zero, and each one is here.
    /// A caller that wants a fallback face walks this; a caller that wants to
    /// know its localisation is incomplete counts it.
    pub fn missing(&self) -> impl Iterator<Item = &PositionedGlyph> {
        self.glyphs.iter().filter(|glyph| glyph.missing)
    }

    /// Move every baseline in the run to `y`.
    pub(crate) fn set_baseline(&mut self, y: I16F16) {
        for glyph in &mut self.glyphs {
            glyph.y = y;
        }
    }
}

/// `text`, shaped at `size` pixels to the em.
///
/// The pen starts at zero and moves by each glyph's advance; before each glyph
/// but the first, the kern for the pair is added, so a kerned pair moves the
/// *second* glyph and leaves the first where it was. Control characters take no
/// glyph and no space, which is what keeps a stray carriage return from setting
/// as a box.
///
/// ```
/// use corvid_fixed::I16F16;
/// use corvid_text::shape;
/// use corvid_ui::Monospace;
///
/// let size = I16F16::from_f64(16.0);
/// let run = shape(&Monospace::DEFAULT, "Ca ira", size);
/// assert_eq!(run.glyphs().len(), 6);
/// // Half the size to the character, six of them.
/// assert_eq!(run.width(), I16F16::from_f64(48.0));
/// assert_eq!(run.glyphs()[1].x, I16F16::from_f64(8.0));
/// // Monospace has every character, so nothing is missing.
/// assert_eq!(run.missing().count(), 0);
/// ```
#[must_use]
pub fn shape<F: Shaping + ?Sized>(font: &F, text: &str, size: I16F16) -> Run {
    let mut glyphs = Vec::with_capacity(text.len());
    let mut pen = I16F16::ZERO;
    let mut previous: Option<GlyphId> = None;
    for (offset, character) in text.char_indices() {
        if character.is_control() {
            continue;
        }
        let found = font.lookup(character);
        let glyph = found.unwrap_or(NOTDEF);
        if let Some(left) = previous {
            pen = pen.saturating_add(font.kern(left, glyph, size));
        }
        glyphs.push(PositionedGlyph {
            glyph,
            x: pen,
            y: I16F16::ZERO,
            cluster: u32::try_from(offset).unwrap_or(u32::MAX),
            character,
            missing: found.is_none(),
        });
        pen = pen.saturating_add(font.advance(glyph, size));
        previous = Some(glyph);
    }
    Run { glyphs, width: pen }
}
