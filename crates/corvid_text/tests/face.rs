//! What a face says about itself, and what it says about French.
//!
//! Every number here is one the test face in `common` decided: a thousand units
//! to the em, five hundred to a letter, two hundred and fifty to the space, and
//! an ascent of eight hundred. At twenty pixels to the em they come out as
//! round numbers, which is the point of choosing them -- an assertion that
//! reads `10.0` is an assertion somebody can check by hand.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use corvid_fixed::I16F16;
use corvid_text::{Font, FontError, NOTDEF, shape};
use corvid_ui::{GlyphId, Metrics as _};

/// Twenty pixels to the em, at which every metric of the test face is exact.
fn size() -> I16F16 {
    I16F16::from_f64(20.0)
}

#[test]
fn bytes_that_are_not_a_face_are_refused() {
    assert_eq!(Font::parse(&[]).err(), Some(FontError::Malformed));
    assert_eq!(Font::parse(b"OTTO").err(), Some(FontError::Malformed));
    // A real header whose tables have been cut off is malformed rather than
    // half-parsed.
    let mut truncated = common::face();
    truncated.truncate(24);
    assert_eq!(Font::parse(&truncated).err(), Some(FontError::Malformed));
}

#[test]
fn a_face_reports_its_own_grid() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    assert_eq!(font.units_per_em(), 1000);
    assert_eq!(font.ascent_units(), 800);
    assert_eq!(font.descent_units(), -200);
    assert_eq!(font.line_units(), 1000);
    assert_eq!(font.line_height(size()), I16F16::from_f64(20.0));
    assert_eq!(font.ascent(size()), I16F16::from_f64(16.0));
}

#[test]
fn every_letter_advances_the_same_and_the_space_advances_half() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let letter = font.glyph('a');
    assert_eq!(font.advance(letter, size()), I16F16::from_f64(10.0));
    assert_eq!(font.advance(font.glyph(' '), size()), I16F16::from_f64(5.0));
}

#[test]
fn the_accents_french_needs_resolve() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    for accented in ['\u{e9}', '\u{e7}'] {
        let glyph = font.lookup(accented).expect("the face maps it");
        assert_ne!(glyph, NOTDEF, "and to a glyph of its own");
        assert_eq!(glyph, GlyphId(u32::from(common::glyph(accented))));
    }
    // The words this is for, set end to end, with nothing missing in them.
    for word in ["R\u{e9}veillon", "faubourg Saint-Antoine", "\u{e7}a ira"] {
        let run = shape(&font, word, size());
        assert_eq!(
            run.missing().count(),
            0,
            "every character of {word} is a glyph the face has"
        );
    }
}

#[test]
fn a_character_the_face_lacks_is_reported_and_still_set() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    assert_eq!(font.lookup(common::ABSENT), None);
    // The trait still has to answer something, and what it answers is the box.
    assert_eq!(font.glyph(common::ABSENT), NOTDEF);

    let word = format!("t{}te", common::ABSENT);
    let run = shape(&font, &word, size());
    assert_eq!(run.glyphs().len(), 4, "the character is set, not dropped");
    let missing: Vec<char> = run.missing().map(|glyph| glyph.character).collect();
    assert_eq!(missing, [common::ABSENT]);
    assert_eq!(run.missing().next().unwrap().glyph, NOTDEF);
    // And it takes the width of the box, so the word does not close up over it.
    assert_eq!(
        run.width(),
        font.advance(NOTDEF, size()).saturating_add(
            font.advance(font.glyph('t'), size())
                .saturating_mul(I16F16::from_f64(3.0))
        )
    );
}

#[test]
fn the_same_string_lays_out_identically_twice() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let text = "faubourg Saint-Antoine";
    assert_eq!(shape(&font, text, size()), shape(&font, text, size()));

    // And across two parses of the same bytes, which is the case that would
    // catch a cache or a pointer leaking into a position.
    let other = Font::parse(&bytes).unwrap();
    assert_eq!(shape(&font, text, size()), shape(&other, text, size()));

    // And at a size that does not divide the em, where every advance truncates.
    let awkward = I16F16::from_f64(13.5);
    assert_eq!(shape(&font, text, awkward), shape(&other, text, awkward));
}

#[test]
fn a_control_character_takes_no_glyph_and_no_space() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let plain = shape(&font, "ab", size());
    let interrupted = shape(&font, "a\u{7}b", size());
    assert_eq!(plain.width(), interrupted.width());
    assert_eq!(plain.glyphs().len(), interrupted.glyphs().len());
}

#[test]
fn a_glyph_carries_the_byte_it_came_from() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    // The acute is two bytes of UTF-8, so the glyph after it starts at four.
    let run = shape(&font, "\u{e9}t\u{e9}", size());
    let clusters: Vec<u32> = run.glyphs().iter().map(|glyph| glyph.cluster).collect();
    assert_eq!(clusters, [0, 2, 3]);
}
