//! The half of text that has no font in it: a bounded line, a glyph number,
//! and the measurements a layout asks a font for.
//!
//! A layout needs three numbers from a font — an advance, a line height and an
//! ascent — and nothing else. [`Metrics`] is those three, so the solver runs
//! against a rasteriser, against a hinted TrueType face, or against
//! [`Monospace`], and the layout it produces is the same shape in each case.

use corvid_fixed::I16F16;
/// A glyph in a font, as that font numbers them.
///
/// Thirty-two bits rather than the sixteen a TrueType face uses, because a
/// [`PaintedGlyph`](crate::PaintedGlyph) is instance data and a `u16` in the
/// middle of one is two bytes of padding a `Pod` derive will not accept.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "bytemuck", derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct GlyphId(pub u32);

impl From<u32> for GlyphId {
    #[inline]
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<GlyphId> for u32 {
    #[inline]
    fn from(id: GlyphId) -> Self {
        id.0
    }
}

/// A line of text was longer than [`Line::CAPACITY`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TooLong {
    /// How many bytes were offered.
    pub bytes: usize,
}

impl core::fmt::Display for TooLong {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{} bytes of text, and a Line holds {}",
            self.bytes,
            Line::CAPACITY
        )
    }
}

impl core::error::Error for TooLong {}

/// A short line of text, stored inline.
///
/// A label costs no allocation and digests to the same bytes on every target,
/// which is what lets a laid-out tree be compared as a `u64`. The unused tail
/// is zero, so two lines holding the same text hold the same bytes.
///
/// ```
/// use corvid_ui::Line;
///
/// let title = Line::new("cradle")?;
/// assert_eq!(title.as_str(), "cradle");
/// assert_eq!(title.len(), 6);
///
/// // Longer than the capacity is refused rather than silently cut.
/// assert!(Line::new(&"x".repeat(Line::CAPACITY + 1)).is_err());
///
/// // Unless the caller asks for a cut, which lands on a character boundary.
/// let long = Line::truncated(&"é".repeat(Line::CAPACITY));
/// assert_eq!(long.len(), Line::CAPACITY);
/// assert_eq!(long.chars().count(), Line::CAPACITY / 2);
/// # Ok::<(), corvid_ui::TooLong>(())
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Line {
    /// The text, zero-filled past `len`.
    bytes: [u8; Self::CAPACITY],
    /// How many of them are text.
    len: u8,
}

impl Line {
    /// How many bytes of UTF-8 a line holds.
    ///
    /// Sixty-four, which is a menu entry, a tooltip, or a console line, and is
    /// not a paragraph. A game with a paragraph builds a column of these.
    pub const CAPACITY: usize = 64;

    /// No text.
    pub const EMPTY: Self = Self {
        bytes: [0; Self::CAPACITY],
        len: 0,
    };

    /// The line holding `text`.
    ///
    /// # Errors
    ///
    /// [`TooLong`] when `text` is more than [`CAPACITY`](Self::CAPACITY) bytes.
    /// Use [`truncated`](Self::truncated) to cut instead.
    pub const fn new(text: &str) -> Result<Self, TooLong> {
        if text.len() > Self::CAPACITY {
            return Err(TooLong { bytes: text.len() });
        }
        Ok(Self::copy(text, text.len()))
    }

    /// The line holding as much of `text` as fits, cut at a character
    /// boundary.
    #[must_use]
    pub const fn truncated(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut len = if bytes.len() < Self::CAPACITY {
            bytes.len()
        } else {
            Self::CAPACITY
        };
        while len > 0 && !boundary(bytes, len) {
            len -= 1;
        }
        Self::copy(text, len)
    }

    /// The first `len` bytes of `text`, which the callers above have already
    /// checked are in range and on a boundary.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "len is at most CAPACITY, which is 64, and both callers establish that before arriving here"
    )]
    const fn copy(text: &str, len: usize) -> Self {
        let source = text.as_bytes();
        let mut bytes = [0; Self::CAPACITY];
        let mut index = 0;
        while index < len {
            bytes[index] = source[index];
            index += 1;
        }
        Self {
            bytes,
            len: len as u8,
        }
    }

    /// The text.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        let (head, _) = self.bytes.split_at(self.len as usize);
        match core::str::from_utf8(head) {
            Ok(text) => text,
            // Unreachable: every constructor copies from a `&str` and cuts on
            // a boundary. Answering the empty line rather than panicking is
            // what keeps this `const` and total.
            Err(_) => "",
        }
    }

    /// How many bytes of text.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// Whether there is no text.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The characters, in order.
    pub fn chars(&self) -> core::str::Chars<'_> {
        self.as_str().chars()
    }
}

/// Whether `index` starts a character, or is the end.
const fn boundary(bytes: &[u8], index: usize) -> bool {
    index >= bytes.len() || (bytes[index] & 0xC0) != 0x80
}

impl Default for Line {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl core::fmt::Debug for Line {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self.as_str(), f)
    }
}

impl core::fmt::Display for Line {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for Line {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl TryFrom<&str> for Line {
    type Error = TooLong;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        Self::new(text)
    }
}

/// What a layout asks a font for.
///
/// Three numbers and a glyph lookup. A font that hints, kerns or shapes
/// answers different numbers and the solver does not change.
pub trait Metrics {
    /// The glyph this font draws for a character.
    fn glyph(&self, character: char) -> GlyphId;

    /// How far the pen moves after drawing `glyph` at `size`.
    fn advance(&self, glyph: GlyphId, size: I16F16) -> I16F16;

    /// Baseline to baseline, at `size`.
    fn line_height(&self, size: I16F16) -> I16F16;

    /// How far the baseline sits below the top of a line box, at `size`.
    fn ascent(&self, size: I16F16) -> I16F16;

    /// How wide `line` is at `size`: the sum of the advances, exactly.
    fn width(&self, line: &Line, size: I16F16) -> I16F16 {
        let mut total = I16F16::ZERO;
        for character in line.chars() {
            total = total.saturating_add(self.advance(self.glyph(character), size));
        }
        total
    }
}

/// The stand-in, and public API rather than a test helper: every glyph is the
/// same width.
///
/// A golden layout wants a font whose numbers are a decision rather than a
/// download, and a game that has not chosen a face yet wants a menu that lays
/// out anyway. Both get this.
///
/// ```
/// use corvid_fixed::I16F16;
/// use corvid_ui::{Line, Metrics as _, Monospace};
///
/// let font = Monospace::DEFAULT;
/// let size = I16F16::from_f64(16.0);
/// // Half the size to the character, so eight characters is sixty-four pixels.
/// assert_eq!(font.width(&Line::new("abcdefgh")?, size), I16F16::from_f64(64.0));
/// # Ok::<(), corvid_ui::TooLong>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Monospace {
    /// The advance of every glyph, as a multiple of the size.
    pub advance: I16F16,
    /// Baseline to baseline, as a multiple of the size.
    pub line: I16F16,
    /// The baseline's depth below the top of the line box, as a multiple of
    /// the line height.
    pub ascent: I16F16,
}

impl Monospace {
    /// Half the size to the character, five quarters of it to the line, and
    /// the baseline three quarters of the way down.
    ///
    /// The proportions of a terminal face, which is what a fallback should
    /// look like: obviously provisional, and wide enough that a menu measured
    /// against it does not overflow when a real face replaces it. All three
    /// are exact in `I16F16`, so a measurement against this font is exact
    /// arithmetic rather than arithmetic that rounds the same way twice.
    pub const DEFAULT: Self = Self {
        advance: I16F16::from_f64(0.5),
        line: I16F16::from_f64(1.25),
        ascent: I16F16::from_f64(0.75),
    };
}

impl Default for Monospace {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Metrics for Monospace {
    fn glyph(&self, character: char) -> GlyphId {
        GlyphId(character as u32)
    }

    fn advance(&self, _glyph: GlyphId, size: I16F16) -> I16F16 {
        size.saturating_mul(self.advance)
    }

    fn line_height(&self, size: I16F16) -> I16F16 {
        size.saturating_mul(self.line)
    }

    fn ascent(&self, size: I16F16) -> I16F16 {
        self.line_height(size).saturating_mul(self.ascent)
    }
}
