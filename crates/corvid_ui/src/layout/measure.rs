//! Pass one: every node's intrinsic size, children before parents.
//!
//! Nothing here places anything. What it produces is the [`Measured`] row per
//! node that `place.rs` distributes free space against.

use core::hash::Hash;

use super::arithmetic::{add, axes, count_as, fixed, length, resolve, sum, wrapped};
use super::{Measured, SLIDER_WIDTH, Solver, TOGGLE_WIDTH, TooLarge};
use crate::{
    arena::NodeId,
    length::Length,
    style::{Axis, Style},
    text::Line,
    widget::Kind,
};
use corvid_fixed::I16F16;

impl<I: Copy + Eq + Hash> Solver<'_, I> {
    /// Pass one: every node's intrinsic size, children before parents.
    pub(super) fn measure(&mut self) -> Result<(), TooLarge> {
        for position in (0..self.order.len()).rev() {
            let id = self.order[position];
            let Some(node) = self.tree.node(id) else {
                continue;
            };
            let style = node.style;
            let font = resolve(
                style.text.size,
                self.scale,
                I16F16::ZERO,
                I16F16::ZERO,
                id,
                Axis::Row,
            )?;
            let border = length(style.border, self.scale, id, Axis::Row)?;
            let frame_x = sum(
                style.padding.horizontal(self.scale),
                border.saturating_mul(I16F16::from_f64(2.0)),
                id,
                Axis::Row,
            )?;
            let frame_y = sum(
                style.padding.vertical(self.scale),
                border.saturating_mul(I16F16::from_f64(2.0)),
                id,
                Axis::Column,
            )?;

            let (width, height) = match node.kind {
                Kind::Label { text, wrap } => {
                    let (w, h) = self.measure_text(&text, wrap, style, font, frame_x);
                    (
                        add(w, frame_x, id, Axis::Row)?,
                        add(h, frame_y, id, Axis::Column)?,
                    )
                }
                Kind::Slider { .. } | Kind::Toggle { .. } => {
                    let wide = if matches!(node.kind, Kind::Slider { .. }) {
                        SLIDER_WIDTH
                    } else {
                        TOGGLE_WIDTH
                    };
                    (
                        add(font.saturating_mul(wide), frame_x, id, Axis::Row)?,
                        add(font, frame_y, id, Axis::Column)?,
                    )
                }
                Kind::Spacer => (frame_x, frame_y),
                Kind::Container | Kind::Button { .. } => {
                    self.measure_children(id, style, frame_x, frame_y)?
                }
            };

            let outer_width = self.outer(id, style, width, Axis::Row)?;
            let outer_height = self.outer(id, style, height, Axis::Column)?;
            self.measured[position] = Measured {
                font,
                width,
                height,
                outer_width,
                outer_height,
            };
        }
        Ok(())
    }

    /// A label's intrinsic content size.
    ///
    /// A wrapped label breaks against the width its own style gives it, which
    /// is the width pass one knows without asking its parent. A label that
    /// wraps and has no width of its own does not wrap, because there is
    /// nothing yet to wrap against.
    fn measure_text(
        &self,
        text: &Line,
        wrap: bool,
        style: Style,
        font: I16F16,
        frame_x: I16F16,
    ) -> (I16F16, I16F16) {
        let line = self.metrics.line_height(font);
        let available = fixed(style.width, self.scale)
            .map(|width| width.saturating_sub(frame_x))
            .filter(|_| wrap)
            .filter(|available| available.to_bits() > 0);
        let Some(available) = available else {
            return (self.metrics.width(text, font), line);
        };
        let lines = wrapped(text, self.metrics, font, available).len().max(1);
        let height = line
            .saturating_mul(count_as(lines))
            .saturating_add(style.text.leading.saturating_mul(count_as(lines - 1)));
        (available, height)
    }

    /// A container's intrinsic border box: its frame, plus its children summed
    /// along its axis and maximised across it.
    fn measure_children(
        &self,
        id: NodeId,
        style: Style,
        frame_x: I16F16,
        frame_y: I16F16,
    ) -> Result<(I16F16, I16F16), TooLarge> {
        let horizontal = style.axis.is_horizontal();
        let (main_axis, cross_axis) = axes(style.axis);
        let gap = length(style.gap, self.scale, id, main_axis)?;
        // The accumulator starts at the frame, so a child that is wider than
        // the type can hold overflows on its own line and names itself.
        let mut main = if horizontal { frame_x } else { frame_y };
        let mut cross = I16F16::ZERO;
        let mut seen = 0;
        for child in self.tree.children(id) {
            let Some(position) = self.position_of(child) else {
                continue;
            };
            let measured = self.measured[position];
            let (child_main, child_cross) = if horizontal {
                (measured.outer_width, measured.outer_height)
            } else {
                (measured.outer_height, measured.outer_width)
            };
            if seen > 0 {
                main = add(main, gap, child, main_axis)?;
            }
            main = add(main, child_main, child, main_axis)?;
            cross = cross.max(child_cross);
            seen += 1;
        }
        let cross = add(
            cross,
            if horizontal { frame_y } else { frame_x },
            id,
            cross_axis,
        )?;
        Ok(if horizontal {
            (main, cross)
        } else {
            (cross, main)
        })
    }

    /// One axis of a node's outer box: its own length, bounded, plus margin.
    pub(super) fn outer(
        &self,
        id: NodeId,
        style: Style,
        content: I16F16,
        axis: Axis,
    ) -> Result<I16F16, TooLarge> {
        let horizontal = axis.is_horizontal();
        let own = if horizontal {
            style.width
        } else {
            style.height
        };
        let resolved = resolve(own, self.scale, I16F16::ZERO, content, id, axis)?;
        let bounded = self.bound(id, style, resolved, I16F16::ZERO, axis)?;
        let margin = if horizontal {
            style.margin.horizontal(self.scale)
        } else {
            style.margin.vertical(self.scale)
        };
        sum(margin, bounded, id, axis)
    }

    /// A size, held between its style's floor and ceiling.
    pub(super) fn bound(
        &self,
        id: NodeId,
        style: Style,
        value: I16F16,
        free: I16F16,
        axis: Axis,
    ) -> Result<I16F16, TooLarge> {
        let horizontal = axis.is_horizontal();
        let (min, max) = if horizontal {
            (style.min.width, style.max.width)
        } else {
            (style.min.height, style.max.height)
        };
        let floor = match min {
            Length::Auto => I16F16::ZERO,
            other => resolve(other, self.scale, free, I16F16::ZERO, id, axis)?,
        };
        let ceiling = match max {
            Length::Auto => I16F16::MAX,
            other => resolve(other, self.scale, free, I16F16::ZERO, id, axis)?,
        };
        Ok(value.clamp(floor, ceiling.max(floor)))
    }
}
