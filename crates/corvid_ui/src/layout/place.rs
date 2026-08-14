//! Pass two: every node's rectangle, parents before children.
//!
//! The seam against `measure.rs` is direction. This walk is top-down and is
//! the only one that knows how much space there is: it reads the intrinsic
//! sizes pass one left, hands the leftover to the `Fraction` children, and
//! writes the rectangle each node ends up with.

use core::hash::Hash;

use super::arithmetic::{add, axes, length, resolve, sum, times};
use super::{Solver, TooLarge};
use crate::{
    paint::{Painted, Rect},
    style::{Axis, Style},
};
use corvid_fixed::I16F16;

impl<I: Copy + Eq + Hash> Solver<'_, I> {
    /// Pass two: every node's rectangle, parents before children, painting
    /// each as it goes.
    pub(super) fn place(&mut self, viewport: Rect, painted: &mut Painted) -> Result<(), TooLarge> {
        let root = self.order[0];
        let Some(style) = self.tree.node(root).map(|node| node.style) else {
            return Ok(());
        };
        let measured = self.measured[0];
        let width = resolve(
            style.width,
            self.scale,
            viewport.width,
            measured.width,
            root,
            Axis::Row,
        )?;
        let height = resolve(
            style.height,
            self.scale,
            viewport.height,
            measured.height,
            root,
            Axis::Column,
        )?;
        self.rect[0] = Rect::new(
            viewport
                .x
                .saturating_add(length(style.margin.left, self.scale, root, Axis::Row)?),
            viewport
                .y
                .saturating_add(length(style.margin.top, self.scale, root, Axis::Column)?),
            self.bound(root, style, width, viewport.width, Axis::Row)?,
            self.bound(root, style, height, viewport.height, Axis::Column)?,
        );

        for position in 0..self.order.len() {
            self.paint(position, painted);
            self.place_children(position, painted)?;
        }
        Ok(())
    }

    /// Lay one node's children out inside its content box.
    fn place_children(&mut self, position: usize, painted: &mut Painted) -> Result<(), TooLarge> {
        let id = self.order[position];
        let Some(style) = self.tree.node(id).map(|node| node.style) else {
            return Ok(());
        };
        let inner = self.content_box(position, style)?;

        let clip = if style.clip {
            let outer = painted
                .clips
                .get(self.clip[position] as usize)
                .copied()
                .unwrap_or(painted.size);
            painted.clips.push(inner.intersection(outer));
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a UI with four billion clip regions is not a case this crate is asked to answer"
            )]
            {
                (painted.clips.len() - 1) as u32
            }
        } else {
            self.clip[position]
        };

        self.gather(id, style)?;
        if self.kids.is_empty() {
            return Ok(());
        }
        let (main_axis, _) = axes(style.axis);
        let inner_main = if style.axis.is_horizontal() {
            inner.width
        } else {
            inner.height
        };
        let gap = length(style.gap, self.scale, id, main_axis)?;
        let gaps = times(gap, self.kids.len() - 1, id, main_axis)?;

        let taken = self.taken(gaps, main_axis)?;
        let free = inner_main.saturating_sub(taken).max(I16F16::ZERO);
        self.share(free, id, main_axis)?;

        let used = self.taken(gaps, main_axis)?;
        let leftover = inner_main.saturating_sub(used).max(I16F16::ZERO);
        self.arrange(style, inner, gap, leftover, clip)
    }

    /// A node's content box: its rectangle, less its border and padding.
    pub(super) fn content_box(&self, position: usize, style: Style) -> Result<Rect, TooLarge> {
        let id = self.order[position];
        let rect = self.rect[position];
        let border = length(style.border, self.scale, id, Axis::Row)?;
        let left = add(
            border,
            length(style.padding.left, self.scale, id, Axis::Row)?,
            id,
            Axis::Row,
        )?;
        let top = add(
            border,
            length(style.padding.top, self.scale, id, Axis::Column)?,
            id,
            Axis::Column,
        )?;
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
        Ok(Rect::new(
            rect.x.saturating_add(left),
            rect.y.saturating_add(top),
            rect.width.saturating_sub(frame_x).max(I16F16::ZERO),
            rect.height.saturating_sub(frame_y).max(I16F16::ZERO),
        ))
    }
}
