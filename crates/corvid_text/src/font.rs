//! A parsed face, and the numbers a layout asks it for.

use crate::FontError;
use crate::kern::pair;
use corvid_fixed::I16F16;
use corvid_ui::{GlyphId, Metrics};
use ttf_parser::{Face, FaceParsingError};

/// A face, parsed, borrowing the bytes it was read from.
///
/// The bytes outlive the font rather than being copied into it: a face is a
/// couple of megabytes and a game has already mapped the file, so a second copy
/// buys nothing. Nothing here allocates until a glyph is rasterised.
///
/// [`Metrics`] is implemented, which is the whole point of the borrow: a
/// `corvid_ui` tree measured against `Monospace` measures against this instead
/// by changing one binding, and the layout it solves has the same shape.
///
/// ```
/// use corvid_text::{Font, FontError};
///
/// // Ten bytes of prose are not a face, and that is reported rather than
/// // guessed at.
/// assert_eq!(Font::parse(b"not a font").err(), Some(FontError::Malformed));
/// ```
#[derive(Clone)]
pub struct Font<'a> {
    /// The tables.
    face: Face<'a>,
    /// The design grid, cached because every measurement divides by it and
    /// because the constructor has already proved it is not zero.
    em: u16,
}

impl<'a> Font<'a> {
    /// The face in `bytes`.
    ///
    /// A collection is read at index zero and the rest of it is ignored; see
    /// the scope section of the crate documentation.
    ///
    /// # Errors
    ///
    /// [`FontError`] when the bytes are not a face, when a table a measurement
    /// needs is absent, or when the face divides its em into nothing.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FontError> {
        let face = Face::parse(bytes, 0).map_err(|error| match error {
            FaceParsingError::MalformedFont | FaceParsingError::UnknownMagic => {
                FontError::Malformed
            }
            FaceParsingError::FaceIndexOutOfBounds => FontError::Empty,
            FaceParsingError::NoHeadTable => FontError::MissingTable { table: "head" },
            FaceParsingError::NoHheaTable => FontError::MissingTable { table: "hhea" },
            FaceParsingError::NoMaxpTable => FontError::MissingTable { table: "maxp" },
        })?;
        let em = face.units_per_em();
        if em == 0 {
            return Err(FontError::NoEmSize);
        }
        Ok(Self { face, em })
    }

    /// How many units the face divides its em square into.
    ///
    /// Never zero: [`parse`](Self::parse) refuses a face that says otherwise,
    /// which is what lets every conversion below divide without checking.
    #[must_use]
    pub const fn units_per_em(&self) -> u16 {
        self.em
    }

    /// How many glyphs the face holds.
    #[must_use]
    pub fn glyphs(&self) -> u16 {
        self.face.number_of_glyphs()
    }

    /// The glyph for `character`, or `None` when the face has none.
    ///
    /// This is the honest half of the character map, and
    /// [`Metrics::glyph`] is the half that has to answer something: the trait
    /// returns [`NOTDEF`](crate::NOTDEF) where this returns `None`.
    #[must_use]
    pub fn lookup(&self, character: char) -> Option<GlyphId> {
        self.cmap(character)
    }

    /// The character map, reached by one private name so that the inherent
    /// method, [`Metrics::glyph`] and [`Shaping::lookup`](crate::Shaping::lookup)
    /// cannot resolve to each other: three public spellings of one lookup, and
    /// two of them share a name.
    fn cmap(&self, character: char) -> Option<GlyphId> {
        self.face
            .glyph_index(character)
            .map(|glyph| GlyphId(u32::from(glyph.0)))
    }

    /// How far the pen moves after drawing `glyph`, in font units.
    ///
    /// Zero for a glyph the face does not number, which is what a face with no
    /// horizontal metrics answers for every glyph.
    #[must_use]
    pub fn advance_units(&self, glyph: GlyphId) -> i32 {
        let Some(id) = narrow(glyph) else {
            return 0;
        };
        self.face.glyph_hor_advance(id).map_or(0, i32::from)
    }

    /// The extra advance the face asks for between `left` and `right`, in font
    /// units, negative where the pair tucks together.
    #[must_use]
    pub fn kern_units(&self, left: GlyphId, right: GlyphId) -> i32 {
        let (Some(left), Some(right)) = (narrow(left), narrow(right)) else {
            return 0;
        };
        i32::from(pair(&self.face, left, right))
    }

    /// The distance from the baseline to the top of the tallest glyph, in font
    /// units.
    #[must_use]
    pub fn ascent_units(&self) -> i32 {
        i32::from(self.face.ascender())
    }

    /// The distance from the baseline down to the bottom of the deepest glyph,
    /// in font units. Negative, as `hhea` writes it.
    #[must_use]
    pub fn descent_units(&self) -> i32 {
        i32::from(self.face.descender())
    }

    /// Baseline to baseline, in font units: the ascent, the descent and the
    /// gap the face asks for between lines.
    #[must_use]
    pub fn line_units(&self) -> i32 {
        self.ascent_units() - self.descent_units() + i32::from(self.face.line_gap())
    }

    /// `units` of the em grid, at `size` pixels to the em.
    ///
    /// The one conversion in this crate, and it is integer throughout: a
    /// 64-bit product divided by the em, truncated toward zero and saturated at
    /// the ends of the fixed-point range. Two machines that agree on the font
    /// bytes agree on this to the bit, which is what a layout needs even though
    /// nothing here is hashed.
    #[must_use]
    pub fn scale(&self, units: i32, size: I16F16) -> I16F16 {
        let product = i64::from(units) * i64::from(size.to_bits());
        I16F16::saturating_from_bits(product / i64::from(self.em))
    }

    /// The face, for a caller that needs a table this crate does not read.
    ///
    /// An escape hatch and named as one. What is behind it is the parser's own
    /// type, so a caller that reaches through it is pinned to the parser this
    /// crate happens to use; everything else here is not.
    #[must_use]
    pub const fn face(&self) -> &Face<'a> {
        &self.face
    }
}

/// A `corvid_ui` glyph number as a TrueType one, or `None` when it is past the
/// sixteen bits a face numbers glyphs in.
///
/// The widening is [`corvid_ui::GlyphId`]'s doing -- it is 32 bits so that
/// instance data has no padding in it -- so the narrowing has to happen
/// somewhere, and here is the only place a glyph number meets a table.
pub(crate) fn narrow(glyph: GlyphId) -> Option<ttf_parser::GlyphId> {
    u16::try_from(glyph.0).ok().map(ttf_parser::GlyphId)
}

/// The tables that were read, rather than the two kilobytes of parsed offsets
/// behind them.
///
/// Hand-written because `ttf_parser::Face` has no `Debug` at all, and because
/// the useful thing to see in a log is what the face measures like: a dump of
/// its table offsets is a dump of the file.
impl core::fmt::Debug for Font<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Font")
            .field("units_per_em", &self.em)
            .field("glyphs", &self.glyphs())
            .field("ascent_units", &self.ascent_units())
            .field("descent_units", &self.descent_units())
            .finish()
    }
}

impl Metrics for Font<'_> {
    /// [`NOTDEF`](crate::NOTDEF) for a character the face has no glyph for, so
    /// that a layout measures the box it is going to draw.
    fn glyph(&self, character: char) -> GlyphId {
        self.cmap(character).unwrap_or(crate::NOTDEF)
    }

    fn advance(&self, glyph: GlyphId, size: I16F16) -> I16F16 {
        self.scale(self.advance_units(glyph), size)
    }

    fn line_height(&self, size: I16F16) -> I16F16 {
        self.scale(self.line_units(), size)
    }

    fn ascent(&self, size: I16F16) -> I16F16 {
        self.scale(self.ascent_units(), size)
    }
}

impl crate::Shaping for Font<'_> {
    fn lookup(&self, character: char) -> Option<GlyphId> {
        self.cmap(character)
    }

    fn kern(&self, left: GlyphId, right: GlyphId, size: I16F16) -> I16F16 {
        self.scale(self.kern_units(left, right), size)
    }
}
