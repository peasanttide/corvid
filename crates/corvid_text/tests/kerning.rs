//! What a kern pair does, and what an unkerned pair does not.
//!
//! The test face kerns exactly two pairs and keeps them in two different
//! tables: `AV` in GPOS, which is where a face cut this century puts them, and
//! `XV` in the legacy `kern` table, which is where a converted one does. Every
//! other pair in the face is silent, which is what makes the negative
//! assertions here mean something.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use corvid_fixed::I16F16;
use corvid_text::{Font, Shaping as _, shape};
use corvid_ui::Metrics as _;

/// Twenty pixels to the em: a letter advances ten, and the two kerns are three
/// pixels and two.
fn size() -> I16F16 {
    I16F16::from_f64(20.0)
}

#[test]
fn a_kern_pair_moves_the_second_glyph_and_not_the_first() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let kerned = shape(&font, "AV", size());
    let plain = shape(&font, "AX", size());

    assert_eq!(kerned.glyphs()[0].x, I16F16::ZERO);
    assert_eq!(
        plain.glyphs()[0].x,
        kerned.glyphs()[0].x,
        "the first glyph of a kerned pair is where it always was"
    );
    assert_eq!(
        plain.glyphs()[1].x,
        I16F16::from_f64(10.0),
        "an unkerned pair is one advance apart"
    );
    assert_eq!(
        kerned.glyphs()[1].x,
        I16F16::from_f64(7.0),
        "and a kerned one is three pixels closer"
    );
}

#[test]
fn a_kern_narrows_the_run_it_is_in() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    assert_eq!(shape(&font, "AX", size()).width(), I16F16::from_f64(20.0));
    assert_eq!(shape(&font, "AV", size()).width(), I16F16::from_f64(17.0));
}

#[test]
fn the_legacy_kern_table_answers_when_gpos_does_not() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    // `XV` is in the `kern` table and nowhere else.
    assert_eq!(
        font.kern_units(font.glyph('X'), font.glyph('V')),
        i32::from(common::XV_KERN)
    );
    assert_eq!(
        shape(&font, "XV", size()).glyphs()[1].x,
        I16F16::from_f64(8.0)
    );
}

#[test]
fn kerning_is_not_symmetric_and_does_not_leak_to_neighbours() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let (a, v, x) = (font.glyph('A'), font.glyph('V'), font.glyph('X'));
    assert_eq!(font.kern_units(a, v), i32::from(common::AV_KERN));
    assert_eq!(font.kern_units(v, a), 0, "the pair is ordered");
    assert_eq!(font.kern_units(a, x), 0, "and it is a pair, not a class");
    assert_eq!(font.kern_units(x, a), 0);
    assert_eq!(font.kern(a, v, size()), I16F16::from_f64(-3.0));
    assert_eq!(font.kern(a, x, size()), I16F16::ZERO);
}

#[test]
fn a_kern_pair_split_across_a_line_break_does_not_kern() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    // Wide enough for one letter and no more, so `A` and `V` land on separate
    // lines and the pair stops being a pair.
    let block = corvid_text::Paragraph::layout(&font, "AV", size(), I16F16::from_f64(10.0));
    assert_eq!(block.rows().len(), 2);
    for row in block.rows() {
        assert_eq!(row.run.glyphs()[0].x, I16F16::ZERO);
    }
}

#[test]
fn a_face_with_no_kerning_at_all_kerns_nothing() {
    // `corvid_ui::Monospace` implements the trait with an empty block, which is
    // the whole point of the defaults: the pair that moves above does not move
    // here, and the code that shapes them is the same code.
    let font = corvid_ui::Monospace::DEFAULT;
    let kerned = shape(&font, "AV", size());
    let plain = shape(&font, "AX", size());
    assert_eq!(kerned.glyphs()[1].x, plain.glyphs()[1].x);
    assert_eq!(kerned.width(), plain.width());
}
