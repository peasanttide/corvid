//! What a glyph looks like, and where it lands on a page.
//!
//! The outlines in the test face are rectangles, which is what makes coverage
//! assertable: the inside of a rectangle is fully covered and the outside is
//! not covered at all, and the only interesting pixels are the ones the edge
//! passes through. A face of curves would need a golden image, and a golden
//! image would test the encoder that wrote it as much as the rasteriser.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use corvid_fixed::I16F16;
use corvid_text::{Atlas, Coverage, Font, NOTDEF};
use corvid_ui::{GlyphId, Metrics as _};

/// Twenty pixels to the em, so a letter is eight pixels wide and fourteen tall.
fn size() -> I16F16 {
    I16F16::from_f64(20.0)
}

/// How much ink there is, as a multiple of a fully covered pixel.
fn ink(coverage: &Coverage) -> f64 {
    coverage
        .pixels()
        .iter()
        .map(|value| f64::from(*value) / 255.0)
        .sum()
}

#[test]
fn a_letter_rasterises_to_the_box_its_outline_encloses() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let a = font.rasterize(font.glyph('A'), size());

    // The outline runs from 50 to 450 across and 0 to 700 up, at fifty units to
    // the pixel: eight pixels by fourteen, give or take the rounding at each
    // edge.
    assert!((7..=9).contains(&a.width()), "width was {}", a.width());
    assert!((13..=15).contains(&a.height()), "height was {}", a.height());
    assert!(a.top() < 0, "a glyph sits above the baseline it is on");
    assert!(a.left() >= 0);

    // A solid rectangle covers its own area, and nothing outside it.
    let area = ink(&a);
    assert!(
        (area - 112.0).abs() < 1.0,
        "{area} pixels of ink, wanted 112"
    );
    assert_eq!(
        a.at(a.width() / 2, a.height() / 2),
        0xff,
        "and the middle of it is solid"
    );
}

#[test]
fn a_space_rasterises_to_nothing_at_all() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let space = font.rasterize(font.glyph(' '), size());
    assert!(space.is_blank());
    assert_eq!(space.pixels(), &[] as &[u8]);
    // And a size of nothing draws nothing rather than dividing by zero.
    assert!(font.rasterize(font.glyph('A'), I16F16::ZERO).is_blank());
}

#[test]
fn a_composite_accent_draws_both_of_its_parts() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let plain = font.rasterize(font.glyph('e'), size());
    let accented = font.rasterize(font.glyph('\u{e9}'), size());

    assert!(
        accented.height() > plain.height(),
        "the acute reaches above the letter"
    );
    assert!(
        accented.top() < plain.top(),
        "and the bitmap starts higher up"
    );
    // The letter is 400 units tall and the acute 200, at fifty units to the
    // pixel: eight by eight and four by four.
    let area = ink(&accented);
    assert!((area - 80.0).abs() < 2.0, "{area} pixels of ink, wanted 80");
}

#[test]
fn the_missing_glyph_is_something_rather_than_nothing() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let box_glyph = font.rasterize(NOTDEF, size());
    assert!(!box_glyph.is_blank(), "a hole has to be visible to be seen");
    assert!(ink(&box_glyph) > 0.0);
}

#[test]
fn rasterising_the_same_glyph_twice_gives_the_same_bytes() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    for character in ['A', 'e', '\u{e9}', '\u{e7}', '-'] {
        let glyph = font.glyph(character);
        let once = font.rasterize(glyph, size());
        let twice = font.rasterize(glyph, size());
        assert_eq!(once, twice, "{character:?} rasterised differently twice");
    }
}

#[test]
fn glyphs_are_packed_side_by_side_with_a_gap_between_them() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let mut atlas = Atlas::new(128, 128, size());
    let a = font.glyph('A');
    let v = font.glyph('V');
    let first = atlas.insert(a, &font.rasterize(a, size())).unwrap();
    let second = atlas.insert(v, &font.rasterize(v, size())).unwrap();

    assert_eq!(first.y, second.y, "both on the first shelf");
    assert!(
        second.x > first.x + first.width,
        "and a pixel apart, so neither bleeds into the other"
    );
    assert_eq!(atlas.len(), 2);
    assert_eq!(atlas.slot(a), Some(first));
    assert_eq!(atlas.slot(font.glyph('e')), None);

    // The page holds the coverage the glyph was rasterised with.
    let drawn = font.rasterize(a, size());
    let row = usize::try_from(first.y + drawn.height() / 2).unwrap();
    let column = usize::try_from(first.x + drawn.width() / 2).unwrap();
    assert_eq!(atlas.pixels()[row * 128 + column], 0xff);
}

#[test]
fn the_same_glyph_inserted_twice_keeps_the_slot_it_had() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let mut atlas = Atlas::new(64, 64, size());
    let a = font.glyph('A');
    let coverage = font.rasterize(a, size());
    let first = atlas.insert(a, &coverage).unwrap();
    let again = atlas.insert(a, &coverage).unwrap();
    assert_eq!(first, again);
    assert_eq!(atlas.len(), 1, "and it is one glyph, not two");
}

#[test]
fn an_atlas_that_fills_reports_it_rather_than_overwriting() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    // Room for one letter and not two, once the padding is counted: a letter is
    // nine pixels by fifteen at this size, so one shelf fits and a second does
    // not.
    let mut atlas = Atlas::new(16, 20, size());
    let a = font.glyph('A');
    let first = atlas.insert(a, &font.rasterize(a, size())).unwrap();

    let mut refused = None;
    for character in "BCDEFGH".chars() {
        let glyph = font.glyph(character);
        if let Err(full) = atlas.insert(glyph, &font.rasterize(glyph, size())) {
            refused = Some((glyph, full));
            break;
        }
    }
    let (glyph, full) = refused.expect("a sixteen pixel page cannot hold eight letters");
    assert_eq!(full.glyph, glyph);
    assert_eq!((full.page_width, full.page_height), (16, 20));
    assert!(full.width > 0 && full.height > 0);

    assert_eq!(
        atlas.slot(a),
        Some(first),
        "and what was already on the page is still where it was"
    );
    assert_eq!(
        atlas.slot(glyph),
        None,
        "and the refused glyph is not on it"
    );
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "the claim is that the rectangle is exactly zero area, which is the one float comparison that is exact"
)]
fn a_glyph_the_page_does_not_hold_samples_nothing() {
    let atlas = Atlas::new(32, 32, size());
    assert_eq!(atlas.uv(GlyphId(7)), [0.0; 4]);
    assert_eq!(atlas.quad(GlyphId(7)), [0.0; 4]);
    assert!(atlas.is_empty());
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "the claim is that the rectangle is exactly zero area, which is the one float comparison that is exact"
)]
fn a_blank_glyph_takes_a_slot_and_no_pixels() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let mut atlas = Atlas::new(8, 8, size());
    let space = font.glyph(' ');
    let slot = atlas
        .insert(space, &font.rasterize(space, size()))
        .expect("a page always has room for nothing");
    assert_eq!((slot.width, slot.height), (0, 0));
    assert_eq!(atlas.slot(space), Some(slot));
    assert_eq!(atlas.uv(space), [0.0; 4], "and it samples nothing");
}
