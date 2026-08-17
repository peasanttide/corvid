//! A TrueType face, assembled byte by byte in the test binary.
//!
//! There is no font file in this repository and there is not going to be one.
//! A downloaded face makes the tests depend on a download; a face committed as
//! a blob makes them depend on a blob nobody can read a diff of; a face taken
//! from the system makes them depend on which machine they run on. A face built
//! here is none of those: every advance, every kern pair and every outline in
//! it is a line of code above, so a test that asserts a number can be read
//! beside the line that decided it.
//!
//! What it holds is Latin: the letters, the space, the hyphen, the apostrophe,
//! and `e` acute and `c` cedilla as *composite* glyphs, which is how a real
//! face spells an accent. It deliberately does not hold `e` circumflex, so that
//! there is something for a missing-glyph test to miss.

#![allow(
    dead_code,
    reason = "each integration test binary includes the whole module and uses the part of it that its own subject needs"
)]

mod tables;

use tables::Writer;

/// Every character the face maps, in glyph order.
///
/// The accented pair sits at the end rather than in code point order, because
/// glyph numbers here follow this string and the character map is what sorts
/// itself out.
pub(crate) const CHARS: &str =
    " ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-'\u{e9}\u{e7}";

/// A character the face does not have, for a test that needs one.
pub(crate) const ABSENT: char = '\u{ea}';

/// The glyph the empty box is.
pub(crate) const NOTDEF: u16 = 0;
/// The acute mark, which the character map does not name: it is reachable only
/// as a component of a composite.
pub(crate) const ACUTE: u16 = 1;
/// The cedilla, likewise.
pub(crate) const CEDILLA: u16 = 2;

/// The advance every letter has, in font units.
pub(crate) const ADVANCE: u16 = 500;
/// The advance the space has.
pub(crate) const SPACE_ADVANCE: u16 = 250;

/// How much the face tucks a `V` under an `A`, in font units. In GPOS.
pub(crate) const AV_KERN: i16 = -150;
/// How much it tucks a `V` under an `X`. In the legacy `kern` table only, so
/// that the fallback has something to find.
pub(crate) const XV_KERN: i16 = -100;

/// What a glyph looks like.
enum Shape {
    /// No outline at all, which is what a space is.
    Empty,
    /// One rectangular contour: `(x_min, y_min, x_max, y_max)`.
    Rect(i16, i16, i16, i16),
    /// Other glyphs, placed: `(glyph, dx, dy)`.
    Composite(Vec<(u16, i16, i16)>),
}

/// The glyph for `character`, or [`NOTDEF`] when the face has none.
pub(crate) fn glyph(character: char) -> u16 {
    CHARS
        .chars()
        .position(|c| c == character)
        .and_then(|index| u16::try_from(index + 3).ok())
        .unwrap_or(NOTDEF)
}

/// Every glyph in the face, in order.
fn glyphs() -> Vec<(u16, Shape)> {
    let mut all = vec![
        (600, Shape::Rect(100, 0, 500, 700)),
        (0, Shape::Rect(150, 500, 350, 700)),
        (0, Shape::Rect(150, -200, 350, -50)),
    ];
    for character in CHARS.chars() {
        all.push(match character {
            ' ' => (SPACE_ADVANCE, Shape::Empty),
            '-' => (ADVANCE, Shape::Rect(50, 300, 450, 360)),
            '\'' => (ADVANCE, Shape::Rect(200, 500, 280, 700)),
            '\u{e9}' => (
                ADVANCE,
                Shape::Composite(vec![(glyph('e'), 0, 0), (ACUTE, 0, 0)]),
            ),
            '\u{e7}' => (
                ADVANCE,
                Shape::Composite(vec![(glyph('c'), 0, 0), (CEDILLA, 0, 0)]),
            ),
            letter if letter.is_ascii_uppercase() => (ADVANCE, Shape::Rect(50, 0, 450, 700)),
            _ => (ADVANCE, Shape::Rect(50, 0, 450, 400)),
        });
    }
    all
}

/// The whole face, as the bytes of a `.ttf` file.
pub(crate) fn face() -> Vec<u8> {
    let glyphs = glyphs();
    let count = u16::try_from(glyphs.len()).unwrap_or(u16::MAX);
    let advances: Vec<u16> = glyphs.iter().map(|(advance, _)| *advance).collect();
    let (glyf, loca) = outlines(&glyphs);
    assemble(&[
        (*b"GPOS", tables::gpos(glyph('A'), &[(glyph('V'), AV_KERN)])),
        (*b"cmap", cmap()),
        (*b"glyf", glyf),
        (*b"head", tables::head()),
        (*b"hhea", tables::hhea(count)),
        (*b"hmtx", tables::hmtx(&advances)),
        (*b"kern", tables::kern(&[(glyph('X'), glyph('V'), XV_KERN)])),
        (*b"loca", loca),
        (*b"maxp", tables::maxp(count)),
    ])
}

/// The `glyf` table and the `loca` offsets into it.
fn outlines(glyphs: &[(u16, Shape)]) -> (Vec<u8>, Vec<u8>) {
    let mut glyf = Writer::new();
    let mut loca = Writer::new();
    for (_, shape) in glyphs {
        loca.u32(u32::try_from(glyf.len()).unwrap_or(u32::MAX));
        match shape {
            Shape::Empty => {}
            Shape::Rect(x0, y0, x1, y1) => rect(&mut glyf, *x0, *y0, *x1, *y1),
            Shape::Composite(parts) => composite(&mut glyf, glyphs, parts),
        }
        glyf.pad();
    }
    loca.u32(u32::try_from(glyf.len()).unwrap_or(u32::MAX));
    loca.pad();
    (glyf.0, loca.0)
}

/// One rectangular contour, all four points on the curve.
///
/// The flags say neither "short" nor "same", so each coordinate is a signed
/// sixteen-bit delta from the one before it, which is the least clever encoding
/// the format has and the one that is easiest to read back.
fn rect(w: &mut Writer, x0: i16, y0: i16, x1: i16, y1: i16) {
    w.i16(1); // numberOfContours
    w.i16(x0);
    w.i16(y0);
    w.i16(x1);
    w.i16(y1);
    w.u16(3); // endPtsOfContours: four points, last index three
    w.u16(0); // instructionLength
    for _ in 0..4 {
        w.u8(0x01); // ON_CURVE_POINT
    }
    for delta in [x0, x1 - x0, 0, x0 - x1] {
        w.i16(delta);
    }
    for delta in [y0, 0, y1 - y0, 0] {
        w.i16(delta);
    }
}

/// A glyph made of other glyphs, which is how an accented letter is spelled.
fn composite(w: &mut Writer, glyphs: &[(u16, Shape)], parts: &[(u16, i16, i16)]) {
    let box_of = |index: u16| -> (i16, i16, i16, i16) {
        match glyphs.get(usize::from(index)).map(|(_, shape)| shape) {
            Some(Shape::Rect(x0, y0, x1, y1)) => (*x0, *y0, *x1, *y1),
            _ => (0, 0, 0, 0),
        }
    };
    let mut bounds = (i16::MAX, i16::MAX, i16::MIN, i16::MIN);
    for (index, dx, dy) in parts {
        let (x0, y0, x1, y1) = box_of(*index);
        bounds.0 = bounds.0.min(x0 + dx);
        bounds.1 = bounds.1.min(y0 + dy);
        bounds.2 = bounds.2.max(x1 + dx);
        bounds.3 = bounds.3.max(y1 + dy);
    }
    w.i16(-1); // numberOfContours: negative means composite
    w.i16(bounds.0);
    w.i16(bounds.1);
    w.i16(bounds.2);
    w.i16(bounds.3);
    for (at, (index, dx, dy)) in parts.iter().enumerate() {
        // ARG_1_AND_2_ARE_WORDS | ARGS_ARE_XY_VALUES, plus MORE_COMPONENTS
        // until the last one.
        let mut flags = 0x0001 | 0x0002;
        if at + 1 < parts.len() {
            flags |= 0x0020;
        }
        w.u16(flags);
        w.u16(*index);
        w.i16(*dx);
        w.i16(*dy);
    }
}

/// The `cmap` table: one format 12 subtable, which is a sorted list of runs.
fn cmap() -> Vec<u8> {
    let mut mapped: Vec<(u32, u16)> = CHARS.chars().map(|c| (c as u32, glyph(c))).collect();
    mapped.sort_unstable();
    let mut groups: Vec<(u32, u32, u16)> = Vec::new();
    for (code, id) in mapped {
        match groups.last_mut() {
            Some(last)
                if last.1 + 1 == code && u32::from(last.2) + (code - last.0) == u32::from(id) =>
            {
                last.1 = code;
            }
            _ => groups.push((code, code, id)),
        }
    }

    let mut w = Writer::new();
    w.u16(0); // version
    w.u16(1); // one encoding record
    w.u16(3); // platformID: Windows
    w.u16(10); // encodingID: full Unicode
    w.u32(12); // offset to the subtable

    w.u16(12); // format
    w.u16(0); // reserved
    w.u32(u32::try_from(16 + groups.len() * 12).unwrap_or(u32::MAX)); // length
    w.u32(0); // language
    w.u32(u32::try_from(groups.len()).unwrap_or(u32::MAX));
    for (first, last, id) in groups {
        w.u32(first);
        w.u32(last);
        w.u32(u32::from(id));
    }
    w.pad();
    w.0
}

/// The sfnt header and the table directory, over tables already sorted by tag.
fn assemble(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let count = u16::try_from(tables.len()).unwrap_or(u16::MAX);
    let selector = count.ilog2();
    let range = 16u16 << selector;
    let mut w = Writer::new();
    w.u32(0x0001_0000); // sfntVersion: TrueType outlines
    w.u16(count);
    w.u16(range);
    w.u16(u16::try_from(selector).unwrap_or(0));
    w.u16(count * 16 - range);

    let mut offset = u32::try_from(12 + tables.len() * 16).unwrap_or(u32::MAX);
    for (tag, data) in tables {
        w.bytes(tag);
        w.u32(0); // checkSum, which nothing here verifies
        w.u32(offset);
        w.u32(u32::try_from(data.len()).unwrap_or(u32::MAX));
        offset += u32::try_from(data.len()).unwrap_or(u32::MAX);
    }
    for (_, data) in tables {
        w.bytes(data);
    }
    w.0
}
