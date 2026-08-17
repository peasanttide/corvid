//! Where a face keeps the distance between two glyphs.
//!
//! Two places, historically. The `kern` table is the old one and is a sorted
//! list of pairs; GPOS is the new one and holds the same information under a
//! feature named `kern`, either as a list of pairs or as a matrix of classes.
//! A face written this century usually has only the second, a face converted
//! from PostScript often has only the first, and a few have both and disagree.
//! GPOS is asked first when it is present, because it is the one a shaper is
//! expected to honour.
//!
//! What is deliberately not here is the rest of GPOS. Mark attachment, cursive
//! joining and contextual positioning are what a script with stacked marks
//! needs; Latin with precomposed accents needs pair adjustment and nothing
//! else, and the scope section of the crate documentation says so.

use ttf_parser::gpos::{PairAdjustment, PositioningSubtable, ValueRecord};
use ttf_parser::opentype_layout::LayoutTable;
use ttf_parser::{Face, GlyphId, Tag};

/// The feature a Latin face keeps its pair kerning under.
const KERN: Tag = Tag::from_bytes(b"kern");

/// How much further apart `left` and `right` sit than their advances alone
/// would put them, in font units.
///
/// Zero when the face says nothing about the pair, which is the answer for the
/// overwhelming majority of pairs in any face.
pub(crate) fn pair(face: &Face<'_>, left: GlyphId, right: GlyphId) -> i16 {
    if let Some(table) = face.tables().gpos
        && let Some(value) = gpos(&table, left, right)
    {
        return value;
    }
    legacy(face, left, right)
}

/// The GPOS `kern` feature, walked lookup by lookup.
///
/// The feature list is read directly rather than through a script and a
/// language system. A face that kerns Latin differently per language exists in
/// principle and the design this crate serves does not have one; picking the
/// first `kern` feature is the behaviour, stated rather than hidden.
fn gpos(table: &LayoutTable<'_>, left: GlyphId, right: GlyphId) -> Option<i16> {
    let feature = table.features.find(KERN)?;
    for index in feature.lookup_indices {
        let Some(lookup) = table.lookups.get(index) else {
            continue;
        };
        for subtable in lookup.subtables.into_iter::<PositioningSubtable<'_>>() {
            if let PositioningSubtable::Pair(adjustment) = subtable
                && let Some(value) = adjust(&adjustment, left, right)
            {
                return Some(value);
            }
        }
    }
    None
}

/// One pair adjustment subtable, in either of the two shapes it comes in.
///
/// Only the advance applied to the *first* glyph is read. A pair adjustment may
/// in principle move the second glyph as well, and a face that does so is
/// describing something other than kerning -- the two records exist for mark
/// positioning, not for the space between two letters.
fn adjust(adjustment: &PairAdjustment<'_>, left: GlyphId, right: GlyphId) -> Option<i16> {
    match adjustment {
        PairAdjustment::Format1 { coverage, sets } => {
            let index = coverage.get(left)?;
            let (first, _) = sets.get(index)?.get(right)?;
            advance(&first)
        }
        PairAdjustment::Format2 {
            coverage,
            classes,
            matrix,
        } => {
            // The coverage is still the gate in format 2: a glyph outside it is
            // not kerned even when its class would have found a cell, because
            // class zero is "everything not otherwise mentioned".
            coverage.get(left)?;
            let (first, _) = matrix.get((classes.0.get(left), classes.1.get(right)))?;
            advance(&first)
        }
    }
}

/// The horizontal advance a value record carries, or `None` when it carries
/// none.
///
/// A record whose `x_advance` is zero is a record that says the pair is not
/// kerned, and answering `None` for it lets the caller fall through to the
/// `kern` table rather than stopping on an adjustment of nothing.
fn advance(record: &ValueRecord<'_>) -> Option<i16> {
    (record.x_advance != 0).then_some(record.x_advance)
}

/// The `kern` table: every horizontal, non-variable subtable, summed.
///
/// Summed rather than first-wins because that is what the table's own
/// definition says: subtables accumulate, which is how a face expresses "this
/// pair, minus that class of exception".
fn legacy(face: &Face<'_>, left: GlyphId, right: GlyphId) -> i16 {
    let Some(table) = face.tables().kern else {
        return 0;
    };
    let mut total: i16 = 0;
    for subtable in table.subtables {
        if !subtable.horizontal || subtable.variable {
            continue;
        }
        if let Some(value) = subtable.glyphs_kerning(left, right) {
            total = total.saturating_add(value);
        }
    }
    total
}
