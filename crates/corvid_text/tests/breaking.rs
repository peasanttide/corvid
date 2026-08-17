//! Where a line stops.
//!
//! The face makes the arithmetic easy on purpose: at twenty pixels to the em a
//! letter is ten pixels and a space is five, so a line of forty pixels holds
//! four letters and the sums in these assertions can be done in the head.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use corvid_fixed::I16F16;
use corvid_text::{Font, Paragraph, shape, wrap};

/// Twenty pixels to the em: ten to a letter, five to a space.
fn size() -> I16F16 {
    I16F16::from_f64(20.0)
}

/// The lines of `text`, as text.
fn lines(font: &Font<'_>, text: &str, width: f64) -> Vec<String> {
    wrap(font, text, size(), I16F16::from_f64(width))
        .into_iter()
        .map(|line| text[line.range].to_owned())
        .collect()
}

#[test]
fn a_line_ends_at_the_last_space_that_fits() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    // "vive" is forty pixels, and the space after it is the forty-fifth.
    assert_eq!(lines(&font, "vive le roi", 40.0), ["vive", "le", "roi"]);
    // Sixty-five holds "vive le" exactly: forty, a space, twenty.
    assert_eq!(lines(&font, "vive le roi", 65.0), ["vive le", "roi"]);
}

#[test]
fn a_word_that_fits_is_never_cut() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    // Sixty-four is one pixel short of "vive le", so the break moves back to
    // the space rather than taking the `l`.
    assert_eq!(lines(&font, "vive le roi", 64.0), ["vive", "le roi"]);
}

#[test]
fn a_word_wider_than_the_line_is_the_one_thing_that_is_cut() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    assert_eq!(lines(&font, "faubourg", 45.0), ["faub", "ourg"]);
    // And the cut lands at the last character that fitted, not one before it.
    assert_eq!(lines(&font, "faubourg", 39.0), ["fau", "bou", "rg"]);
}

#[test]
fn a_single_character_wider_than_the_line_still_makes_progress() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    // A letter is ten pixels and the line is five, so nothing fits anywhere and
    // the answer has to be one letter to the line rather than an empty loop.
    assert_eq!(lines(&font, "roi", 5.0), ["r", "o", "i"]);
}

#[test]
fn a_newline_ends_a_line_wherever_it_is() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    assert_eq!(lines(&font, "ca\nira", 1000.0), ["ca", "ira"]);
    assert_eq!(lines(&font, "ca\nira", 25.0), ["ca", "ir", "a"]);
}

#[test]
fn the_run_of_spaces_at_a_break_is_eaten() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    assert_eq!(lines(&font, "vive  le  roi", 40.0), ["vive", "le", "roi"]);
}

#[test]
fn a_measured_line_is_the_width_of_the_line_it_names() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let text = "AV faubourg Saint-Antoine";
    // Every break reports a width, and shaping the bytes it names has to agree
    // with it -- including over the kerned pair, which is the case a measure
    // that forgot about kerning would get wrong.
    for line in wrap(&font, text, size(), I16F16::from_f64(70.0)) {
        assert_eq!(
            line.width,
            shape(&font, &text[line.range.clone()], size()).width(),
            "the width of {:?}",
            &text[line.range]
        );
    }
}

#[test]
fn a_paragraph_is_as_tall_as_its_lines_and_as_wide_as_its_widest() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let block = Paragraph::layout(&font, "vive le roi", size(), I16F16::from_f64(40.0));
    assert_eq!(block.rows().len(), 3);
    // Three lines at a line height of twenty.
    assert_eq!(block.height(), I16F16::from_f64(60.0));
    // "vive" is the widest at forty, and the box it was given was also forty.
    assert_eq!(block.width(), I16F16::from_f64(40.0));
    // The first baseline is the ascent, and each one after it a line lower.
    let baselines: Vec<I16F16> = block.rows().iter().map(|row| row.baseline).collect();
    assert_eq!(
        baselines,
        [
            I16F16::from_f64(16.0),
            I16F16::from_f64(36.0),
            I16F16::from_f64(56.0)
        ]
    );
    // And every glyph on a row sits on that row's baseline.
    for row in block.rows() {
        for glyph in row.run.glyphs() {
            assert_eq!(glyph.y, row.baseline);
        }
    }
}

#[test]
fn a_paragraph_measures_the_text_rather_than_the_box() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let block = Paragraph::layout(&font, "roi", size(), I16F16::from_f64(1000.0));
    assert_eq!(block.rows().len(), 1);
    assert_eq!(block.width(), I16F16::from_f64(30.0));
}

#[test]
fn wrapping_the_same_text_twice_gives_the_same_lines() {
    let bytes = common::face();
    let font = Font::parse(&bytes).unwrap();
    let text = "R\u{e9}veillon et le faubourg Saint-Antoine";
    let once = Paragraph::layout(&font, text, size(), I16F16::from_f64(97.0));
    let twice = Paragraph::layout(&font, text, size(), I16F16::from_f64(97.0));
    assert_eq!(once, twice);
    assert!(once.rows().len() > 2, "and it is a paragraph, not a line");
}
