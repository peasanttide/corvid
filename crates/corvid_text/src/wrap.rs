//! Where a line of text stops.

use crate::shape::{Run, Shaping, shape};
use alloc::vec::Vec;
use core::ops::Range;
use corvid_fixed::I16F16;

/// One line, as a slice of the source text and the width it sets to.
///
/// The range excludes the space or the newline that ended the line, so
/// `text[line.range]` is the line as it is drawn and never has a trailing
/// space in it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Break {
    /// The bytes of the source string this line covers.
    pub range: Range<usize>,
    /// How wide it sets, kerning included.
    pub width: I16F16,
}

/// `text`, broken into lines no wider than `max_width`.
///
/// Greedy, and the greed is the specification: a line ends at the last space
/// that still fits, and a word is never cut. The one exception is a word that
/// is wider than the whole line, which has to be cut somewhere or it would
/// never fit anywhere; it is cut at the last character that fits, and if even
/// one character does not fit it takes a line of its own rather than looping
/// forever. A newline in the text ends a line wherever it is.
///
/// ```
/// use corvid_fixed::I16F16;
/// use corvid_text::wrap;
/// use corvid_ui::Monospace;
///
/// let size = I16F16::from_f64(10.0);
/// // Five characters to the line, at five pixels each.
/// let width = I16F16::from_f64(25.0);
/// let lines = wrap(&Monospace::DEFAULT, "the tocsin rang", size, width);
/// let text = |line: &corvid_text::Break| "the tocsin rang"[line.range.clone()].to_owned();
/// assert_eq!(lines.iter().map(text).collect::<Vec<_>>(), ["the", "tocsi", "n", "rang"]);
/// ```
#[must_use]
pub fn wrap<F: Shaping + ?Sized>(
    font: &F,
    text: &str,
    size: I16F16,
    max_width: I16F16,
) -> Vec<Break> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start <= text.len() {
        let Some(rest) = text.get(start..) else {
            break;
        };
        let line = one(font, rest, size, max_width);
        lines.push(Break {
            range: start..start + line.length,
            width: line.width,
        });
        // The scan restarts from the break rather than carrying its state
        // across it, because the first glyph of a new line is not kerned
        // against the last glyph of the old one -- they are not adjacent.
        if line.next == 0 {
            break;
        }
        start += line.next;
        if start >= text.len() {
            break;
        }
    }
    lines
}

/// Where one line ends, measured from the start of `rest`.
struct Cut {
    /// How many bytes of `rest` the line's text covers.
    length: usize,
    /// Where the next line starts, which is past the space or the newline that
    /// ended this one.
    next: usize,
    /// How wide the line sets.
    width: I16F16,
}

/// The first line of `rest`.
fn one<F: Shaping + ?Sized>(font: &F, rest: &str, size: I16F16, max_width: I16F16) -> Cut {
    let mut pen = I16F16::ZERO;
    let mut previous = None;
    // The last run of spaces that fitted: where its line ends, where the next
    // begins, and how wide the line was before the first space was added.
    let mut space: Option<(usize, usize, I16F16)> = None;
    // Whether the character before this one was a space, so that only the first
    // space of a run is a break point and a line never ends on the second.
    let mut spaced = false;
    for (offset, character) in rest.char_indices() {
        if character == '\n' {
            return Cut {
                length: offset,
                next: offset + 1,
                width: pen,
            };
        }
        if character.is_control() {
            continue;
        }
        let glyph = font.glyph(character);
        let mut step = font.advance(glyph, size);
        if let Some(left) = previous {
            step = step.saturating_add(font.kern(left, glyph, size));
        }
        let next = pen.saturating_add(step);
        if next > max_width {
            if character == ' ' {
                // The space itself is what overflowed, so the line ends here
                // rather than one character earlier: the next line would
                // otherwise begin with the space nobody can see.
                return Cut {
                    length: offset,
                    next: skip(rest, offset),
                    width: pen,
                };
            }
            if let Some((end, after, width)) = space {
                return Cut {
                    length: end,
                    next: after,
                    width,
                };
            }
            // A word wider than the line, cut at the last character that fit.
            // `offset` is zero only when the very first character overflows, and
            // then the character is emitted anyway: a line that took nothing
            // would be asked for again forever.
            let length = if offset == 0 {
                character.len_utf8()
            } else {
                offset
            };
            return Cut {
                length,
                next: length,
                width: if offset == 0 { next } else { pen },
            };
        }
        if character == ' ' && !spaced {
            space = Some((offset, skip(rest, offset), pen));
        }
        spaced = character == ' ';
        pen = next;
        previous = Some(glyph);
    }
    Cut {
        length: rest.len(),
        next: rest.len(),
        width: pen,
    }
}

/// Past the run of spaces starting at `offset`.
///
/// The whole run is eaten rather than one space, so that a double space between
/// sentences does not begin the next line with an indent nobody asked for.
fn skip(rest: &str, offset: usize) -> usize {
    rest.get(offset..).map_or(rest.len(), |tail| {
        offset + tail.len() - tail.trim_start_matches(' ').len()
    })
}

/// A block of text, wrapped and shaped: every glyph in it knows where it goes,
/// and the block knows how large it is.
///
/// This is what a `corvid_ui` box asks for. The width is the widest line rather
/// than the width it was given, so a label in a generously sized box measures
/// as the label rather than as the box.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Paragraph {
    /// The lines, top to bottom.
    rows: Vec<Row>,
    /// The widest line.
    width: I16F16,
    /// Line height times the number of lines.
    height: I16F16,
}

/// One line of a [`Paragraph`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Row {
    /// The bytes of the source string this line covers.
    pub range: Range<usize>,
    /// Its glyphs, positioned, with `y` already on this line's baseline.
    pub run: Run,
    /// Where its baseline sits, down from the top of the paragraph.
    pub baseline: I16F16,
}

impl Paragraph {
    /// `text`, wrapped to `max_width` and shaped at `size`.
    ///
    /// ```
    /// use corvid_fixed::I16F16;
    /// use corvid_text::Paragraph;
    /// use corvid_ui::Monospace;
    ///
    /// let size = I16F16::from_f64(10.0);
    /// let text = "vive le tiers etat";
    /// let block = Paragraph::layout(&Monospace::DEFAULT, text, size, I16F16::from_f64(60.0));
    /// assert_eq!(block.rows().len(), 2);
    /// assert_eq!(&text[block.rows()[0].range.clone()], "vive le");
    /// // Two lines at five quarters of the size.
    /// assert_eq!(block.height(), I16F16::from_f64(25.0));
    /// // Measured as the text rather than as the box it was given.
    /// assert_eq!(block.width(), I16F16::from_f64(50.0));
    /// ```
    #[must_use]
    pub fn layout<F: Shaping + ?Sized>(
        font: &F,
        text: &str,
        size: I16F16,
        max_width: I16F16,
    ) -> Self {
        let line_height = font.line_height(size);
        let ascent = font.ascent(size);
        let mut rows = Vec::new();
        let mut width = I16F16::ZERO;
        let mut height = I16F16::ZERO;
        let mut baseline = ascent;
        for line in wrap(font, text, size, max_width) {
            let Some(slice) = text.get(line.range.clone()) else {
                continue;
            };
            let mut run = shape(font, slice, size);
            run.set_baseline(baseline);
            width = width.max(run.width());
            rows.push(Row {
                range: line.range,
                run,
                baseline,
            });
            baseline = baseline.saturating_add(line_height);
            height = height.saturating_add(line_height);
        }
        Self {
            rows,
            width,
            height,
        }
    }

    /// The lines, top to bottom.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The widest line.
    #[must_use]
    pub const fn width(&self) -> I16F16 {
        self.width
    }

    /// How tall the block is: the line height times the number of lines.
    #[must_use]
    pub const fn height(&self) -> I16F16 {
        self.height
    }
}
