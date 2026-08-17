//! What can go wrong between a byte slice and a glyph.

use corvid_ui::GlyphId;

/// The bytes offered to [`Font::parse`](crate::Font::parse) are not a face
/// this crate can read.
///
/// The parser's own error type is deliberately not what is named here. A face
/// is read by `ttf-parser` today, and which crate reads it is this crate's
/// business rather than its caller's; a variant that named a foreign enum would
/// make swapping the parser a breaking change to everybody downstream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum FontError {
    /// The bytes do not describe a face at all, or describe one whose tables
    /// contradict each other.
    #[error("the bytes are not a font face this crate can read")]
    Malformed,
    /// A required table is absent. A face with no `head` has no units per em,
    /// one with no `hhea` has no line height, and one with no `maxp` does not
    /// say how many glyphs it holds.
    #[error("the face has no {table} table")]
    MissingTable {
        /// The four-character table tag, as the specification spells it.
        table: &'static str,
    },
    /// A collection that holds no faces, so there was nothing at index zero.
    #[error("the file is a font collection and holds no faces")]
    Empty,
    /// The face declares zero units per em, which would divide every
    /// measurement by zero.
    #[error("the face declares zero units per em")]
    NoEmSize,
}

/// A glyph did not fit on the page.
///
/// Reported rather than absorbed: an atlas that quietly dropped a glyph would
/// draw the wrong picture, and one that overwrote a neighbour to make room
/// would draw two wrong pictures. The caller's answer is a larger page, a
/// second page, or a smaller size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[error("glyph {} needs {width}x{height} and the {page_width}x{page_height} page has no room left", glyph.0)]
pub struct AtlasFull {
    /// The glyph that did not fit.
    pub glyph: GlyphId,
    /// How wide it is, in pixels, padding excluded.
    pub width: u32,
    /// How tall it is, in pixels, padding excluded.
    pub height: u32,
    /// How wide the page is.
    pub page_width: u32,
    /// How tall the page is.
    pub page_height: u32,
}
