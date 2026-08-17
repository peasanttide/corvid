# `corvid_text`

Glyphs, laid out, with no device in sight: a parsed face, a coverage bitmap, an
atlas page, a kerned run in fixed point, and the line breaks under it.

```rust
use corvid_fixed::I16F16;
use corvid_text::Paragraph;
use corvid_ui::Monospace;

let size = I16F16::from_f64(10.0);
let text = "the faubourg Saint-Antoine";
// Nine characters to the line, at five pixels each.
let block = Paragraph::layout(&Monospace::DEFAULT, text, size, I16F16::from_f64(45.0));
assert_eq!(&text[block.rows()[0].range.clone()], "the");
assert_eq!(&text[block.rows()[1].range.clone()], "faubourg");
// A word longer than the line is the one case that cuts inside a word.
assert_eq!(&text[block.rows()[2].range.clone()], "Saint-Ant");
assert_eq!(&text[block.rows()[3].range.clone()], "oine");
assert_eq!(block.width(), I16F16::from_f64(45.0));
```

`no_std` plus `alloc`, and it names no graphics library and no operating system:
it builds for `thumbv7em-none-eabi`, where a face is parsed in place out of
borrowed bytes and nothing allocates until a glyph is rasterised.
Nothing here draws: [`Atlas`] hands out a page of coverage bytes and where each
glyph sits on it, and turning that into a texture is the device ring's job, as
`corvid_ui_render::Atlas` says in as many words. This crate is the layer under
`corvid_ui`: [`Font`] implements [`corvid_ui::Metrics`], so a widget tree
measured against `corvid_ui::Monospace` measures against a real face by changing
one binding, and [`Shaping`] adds the two things a shaper needs that a box
solver does not -- whether the face has the character, and how a pair of glyphs
move together.

## The seam between fixed point and float

A position is [`corvid_fixed::I16F16`] and coverage is `f32`, and the line
between them is the whole design. Where a glyph goes decides how wide a text box
is, which decides where the button next to it lands, so a position is computed
in integers and comes out the same on every machine. What a glyph looks like is
a number a sampler reads, and a step of 1/255 either way is a picture nobody can
tell apart, so [`Font::rasterize`] is float and is downstream of every
measurement rather than upstream of one.

[`Font::scale`] is the only conversion: font units times the size, as a 64-bit
product, divided by the units per em, truncated toward zero. Two machines that
agree on the bytes of the face agree on every advance to the bit, even though
nothing in this ring is hashed and nothing in it is sent.

## Kerning

[`shape`] moves the pen by each glyph's advance, and before every glyph but the
first it adds the kern for the pair -- so a kerned pair moves the *second* glyph
and leaves the first where it was.

```rust
use corvid_fixed::I16F16;
use corvid_text::{Shaping, shape};
use corvid_ui::{GlyphId, Metrics, Monospace};

/// A monospaced face that tucks a V under an A and does nothing else.
struct Tucked;

impl Shaping for Tucked {
    fn kern(&self, left: GlyphId, right: GlyphId, size: I16F16) -> I16F16 {
        let pair = (u32::from(left), u32::from(right));
        if pair == (u32::from('A'), u32::from('V')) {
            size.saturating_mul(I16F16::from_f64(-0.125))
        } else {
            I16F16::ZERO
        }
    }
}

# impl Metrics for Tucked {
#     fn glyph(&self, c: char) -> GlyphId { Monospace::DEFAULT.glyph(c) }
#     fn advance(&self, g: GlyphId, s: I16F16) -> I16F16 { Monospace::DEFAULT.advance(g, s) }
#     fn line_height(&self, s: I16F16) -> I16F16 { Monospace::DEFAULT.line_height(s) }
#     fn ascent(&self, s: I16F16) -> I16F16 { Monospace::DEFAULT.ascent(s) }
# }
let size = I16F16::from_f64(16.0);
let tucked = shape(&Tucked, "AV", size);
let apart = shape(&Tucked, "AX", size);
assert_eq!(tucked.glyphs()[0].x, apart.glyphs()[0].x, "the first glyph does not move");
assert_eq!(apart.glyphs()[1].x, I16F16::from_f64(8.0));
assert_eq!(tucked.glyphs()[1].x, I16F16::from_f64(6.0), "and the second does");
```

A face keeps its pairs in one of two places and this crate reads both. GPOS,
under the feature tagged `kern`, is asked first, because it is where a face cut
this century puts them and because a shaper is expected to honour it; the legacy
`kern` table answers when GPOS says nothing. Only pair adjustment is read. The
rest of GPOS -- mark attachment, cursive joining, contextual positioning -- is
what a script with stacked marks needs, and Latin with precomposed accents does
not use any of it.

## Missing glyphs are visible and are reported

A character the face has no glyph for is set as glyph zero, the empty box, and
recorded. It is never dropped: a word that quietly lost a letter is a bug that
ships, and a word with a box in it is a bug somebody files.

```rust
use corvid_fixed::I16F16;
use corvid_text::{NOTDEF, Shaping, shape};
use corvid_ui::{GlyphId, Metrics};

/// A face with the unaccented Latin letters and nothing else.
struct Ascii;

impl Shaping for Ascii {
    fn lookup(&self, character: char) -> Option<GlyphId> {
        character.is_ascii().then(|| GlyphId(character as u32))
    }
}

# impl Metrics for Ascii {
#     fn glyph(&self, c: char) -> GlyphId { self.lookup(c).unwrap_or(NOTDEF) }
#     fn advance(&self, _: GlyphId, s: I16F16) -> I16F16 { s }
#     fn line_height(&self, s: I16F16) -> I16F16 { s }
#     fn ascent(&self, s: I16F16) -> I16F16 { s }
# }
let run = shape(&Ascii, "Re\u{301}veillon", I16F16::ONE);
assert_eq!(run.glyphs().len(), 10, "every character is set, including the one that is missing");
assert_eq!(run.missing().count(), 1);
assert_eq!(run.missing().next().map(|glyph| glyph.character), Some('\u{301}'));
assert_eq!(run.missing().next().map(|glyph| glyph.glyph), Some(NOTDEF));
```

That is also the argument for precomposed accents. `Reveillon` set with a
combining acute needs a face that positions marks; set with `U+00E9`, which is
what a French keyboard and every French text file produce, it needs one glyph
the face already has. This crate reads the character map and nothing else, so it
sets the second correctly and reports the first honestly.

## Reading a face

[`Font::parse`] borrows the bytes rather than copying them, and refuses what it
cannot read rather than guessing.

```rust
use corvid_text::{Font, FontError};

assert_eq!(Font::parse(b"not a font").err(), Some(FontError::Malformed));
```

The parsing itself is `ttf-parser`'s, which was chosen because it does exactly
this and nothing else: it reads the tables of a TrueType or OpenType face and
answers what they say, with no rasteriser, no shaper, no layout engine, no
allocator and no `unsafe` inside it, and it builds without an operating system.
The crates that also rasterise -- `fontdue`, `ab_glyph` -- bring their own idea
of layout and their own float positions, which is the half of the problem this
crate exists to do differently; `rustybuzz` and `swash` bring a complete Unicode
shaper, which is several megabytes of tables to set a language that needs a
character map. The parser's own types stay out of the public surface, behind
[`Font::face`], so that replacing it is not a breaking change to anybody.

## Scope

What this covers is a Latin face, laid out horizontally, left to right, in the
accents French uses: `U+00C0` through `U+00FF` resolve if the face has them, and
`Reveillon`, `faubourg Saint-Antoine` and `Ca ira` set with the acute, the
cedilla and the circumflex where they belong. Line breaking is greedy on spaces
and on explicit newlines. Packing is a shelf packer on a single page, and a page
that fills says so.

What it will not cover: bidirectional text, vertical writing, and the shaping a
script with contextual forms or stacked marks needs -- Arabic, Devanagari and
Thai are a Unicode shaper's problem and a Unicode shaper is a different crate.
Nor ligatures, nor hinting, nor subpixel positioning, nor a font collection past
its first face, nor colour glyphs, nor a fallback chain across several faces:
[`Run::missing`] is the report a fallback chain would be built on, and building
it is the caller's decision rather than this crate's default. Nor uploading
anything: a page of bytes is where this crate stops.
