//! What a placed node contributes to the paint data.
//!
//! The seam against `place.rs` is that nothing here decides anything: the
//! rectangle is already fixed, and this turns it into the rectangles and
//! glyphs a renderer draws.

use core::hash::Hash;

use super::Solver;
use super::arithmetic::{count_as, length, wrapped};
use crate::{
    paint::{Painted, PaintedGlyph, PaintedNode, PaintedRect, Position, Rect},
    style::Axis,
    text::Line,
    widget::Kind,
};
use corvid_fixed::I16F16;

impl<I: Copy + Eq + Hash> Solver<'_, I> {
    /// Everything one node contributes to the paint data.
    pub(super) fn paint(&self, position: usize, painted: &mut Painted) {
        let id = self.order[position];
        let Some(node) = self.tree.node(id) else {
            return;
        };
        let style = node.style;
        let rect = self.rect[position];
        let clip = self.clip[position];
        painted.nodes.push(PaintedNode {
            node: id,
            rect,
            focusable: style.focusable,
            clip,
        });

        let border = length(style.border, self.scale, id, Axis::Row).unwrap_or(I16F16::ZERO);
        let corner = length(style.corner, self.scale, id, Axis::Row).unwrap_or(I16F16::ZERO);
        let outlined = border.to_bits() > 0 && style.border_colour.a > 0;
        if style.background.a > 0 || outlined {
            painted.rects.push(PaintedRect {
                rect,
                fill: style.background,
                border: style.border_colour,
                border_width: if outlined { border } else { I16F16::ZERO },
                corner,
                clip,
            });
        }

        let inner = self.content_box(position, style).unwrap_or(Rect::ZERO);
        match node.kind {
            Kind::Label { text, wrap } => self.paint_text(position, &text, wrap, inner, painted),
            Kind::Slider { value, .. } => {
                let filled = crate::length::saturating_scale(inner.width, value);
                painted.rects.push(PaintedRect {
                    rect: Rect::new(inner.x, inner.y, filled, inner.height),
                    fill: style.foreground,
                    border: style.border_colour,
                    border_width: I16F16::ZERO,
                    corner,
                    clip,
                });
            }
            Kind::Toggle { on, .. } => {
                let knob = I16F16::from_bits(inner.width.to_bits() / 2);
                let x = if on {
                    inner.x.saturating_add(knob)
                } else {
                    inner.x
                };
                painted.rects.push(PaintedRect {
                    rect: Rect::new(x, inner.y, knob, inner.height),
                    fill: style.foreground,
                    border: style.border_colour,
                    border_width: I16F16::ZERO,
                    corner,
                    clip,
                });
            }
            Kind::Container | Kind::Button { .. } | Kind::Spacer => {}
        }
    }

    /// One label's glyphs, on one baseline or several.
    fn paint_text(
        &self,
        position: usize,
        text: &Line,
        wrap: bool,
        inner: Rect,
        painted: &mut Painted,
    ) {
        let style = self.style_of(position);
        let font = self.measured[position].font;
        let clip = self.clip[position];
        let line_height = self.metrics.line_height(font);
        let ascent = self.metrics.ascent(font);
        let lines = if wrap && inner.width.to_bits() > 0 {
            wrapped(text, self.metrics, font, inner.width)
        } else {
            alloc::vec![(0, text.len())]
        };
        let step = line_height.saturating_add(style.text.leading);
        for (row, (start, end)) in lines.into_iter().enumerate() {
            let Some(slice) = text.as_str().get(start..end) else {
                continue;
            };
            let baseline = inner
                .y
                .saturating_add(ascent)
                .saturating_add(step.saturating_mul(count_as(row)));
            let mut pen = inner.x;
            for character in slice.chars() {
                let glyph = self.metrics.glyph(character);
                painted.glyphs.push(PaintedGlyph {
                    at: Position::new(pen, baseline),
                    glyph,
                    size: font,
                    tint: style.foreground,
                    clip,
                });
                pen = pen.saturating_add(self.metrics.advance(glyph, font));
            }
        }
    }
}
