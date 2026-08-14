//! How the leftover space along a container's axis is divided.
//!
//! The seam against `place.rs` is that nothing here writes a rectangle: this
//! works out how long each child is along the main axis, one redistribution
//! round included, and `place.rs` is what turns those lengths into positions.

use alloc::vec::Vec;
use core::hash::Hash;

use super::arithmetic::{add, axes, justify_lead, length, resolve};
use super::{Kid, Solver, TooLarge};
use crate::{
    arena::NodeId,
    length::{Length, scale_exactly, split},
    paint::Rect,
    style::{Align, Axis, Style},
};
use corvid_fixed::{Factor16, I16F16};

impl<I: Copy + Eq + Hash> Solver<'_, I> {
    /// Collect a node's children, with their fixed sizes already resolved.
    pub(super) fn gather(&mut self, id: NodeId, style: Style) -> Result<(), TooLarge> {
        let horizontal = style.axis.is_horizontal();
        let (main_axis, _) = axes(style.axis);
        self.kids.clear();
        let positions: Vec<usize> = self
            .tree
            .children(id)
            .filter_map(|child| self.position_of(child))
            .collect();
        for position in positions {
            let child = self.order[position];
            let Some(child_style) = self.tree.node(child).map(|node| node.style) else {
                continue;
            };
            let measured = self.measured[position];
            let (own, content, margin, margin_start) = if horizontal {
                (
                    child_style.width,
                    measured.width,
                    child_style.margin.horizontal(self.scale),
                    child_style.margin.left,
                )
            } else {
                (
                    child_style.height,
                    measured.height,
                    child_style.margin.vertical(self.scale),
                    child_style.margin.top,
                )
            };
            let main = if own.is_fraction() {
                I16F16::ZERO
            } else {
                let resolved = resolve(own, self.scale, I16F16::ZERO, content, child, main_axis)?;
                self.bound(child, child_style, resolved, I16F16::ZERO, main_axis)?
            };
            self.kids.push(Kid {
                at: position,
                main,
                margin: margin.ok_or(TooLarge {
                    node: child,
                    axis: main_axis,
                })?,
                margin_start: length(margin_start, self.scale, child, main_axis)?,
                share: own.share(),
                clamped: false,
            });
        }
        Ok(())
    }

    /// How much of the axis the children and the gaps take right now.
    pub(super) fn taken(&self, gaps: I16F16, axis: Axis) -> Result<I16F16, TooLarge> {
        let mut total = gaps;
        for kid in &self.kids {
            total = add(total, kid.main, self.order[kid.at], axis)?;
            total = add(total, kid.margin, self.order[kid.at], axis)?;
        }
        Ok(total)
    }

    /// Give the free space to the children that asked for a share of it.
    ///
    /// The shares are accumulated rather than divided one at a time, so three
    /// thirds of a hundred pixels fill exactly a hundred and the remainder
    /// lands on the last of them. A child whose bounds cut its share down
    /// returns the difference, which goes round once more among the children
    /// that were not cut. Once, and not to a fixed point.
    pub(super) fn share(&mut self, free: I16F16, id: NodeId, axis: Axis) -> Result<(), TooLarge> {
        let whole = u32::from(Factor16::MAX.to_bits());
        let raw = self.distribute(free, |kid| kid.share, whole, id, axis)?;
        let mut returned = I16F16::ZERO;
        let mut settled = Vec::with_capacity(self.kids.len());
        for (kid, want) in self.kids.iter().zip(&raw) {
            settled.push(if kid.share == Factor16::ZERO {
                kid.main
            } else {
                self.bound(self.order[kid.at], self.style_of(kid.at), *want, free, axis)?
            });
        }
        for ((kid, want), bounded) in self.kids.iter_mut().zip(&raw).zip(settled) {
            if kid.share == Factor16::ZERO {
                continue;
            }
            kid.main = bounded;
            if bounded != *want {
                kid.clamped = true;
                returned = returned.saturating_add(want.saturating_sub(bounded));
            }
        }
        if returned.to_bits() <= 0 {
            return Ok(());
        }

        // The children that were not cut share the whole of what was returned
        // between them, so their shares are read against their own sum rather
        // than against the whole axis: two halves, one of them clamped, means
        // the other takes all of it and not half of it.
        let remaining: u32 = self
            .kids
            .iter()
            .filter(|kid| !kid.clamped)
            .map(|kid| u32::from(kid.share.to_bits()))
            .sum();
        if remaining == 0 {
            return Ok(());
        }
        let again = self.distribute(
            returned,
            |kid| {
                if kid.clamped {
                    Factor16::ZERO
                } else {
                    kid.share
                }
            },
            remaining,
            id,
            axis,
        )?;
        let mut settled = Vec::with_capacity(self.kids.len());
        for (kid, extra) in self.kids.iter().zip(&again) {
            let want = kid.main.saturating_add(*extra);
            settled.push(if kid.share == Factor16::ZERO || kid.clamped {
                kid.main
            } else {
                self.bound(self.order[kid.at], self.style_of(kid.at), want, free, axis)?
            });
        }
        for (kid, bounded) in self.kids.iter_mut().zip(settled) {
            kid.main = bounded;
        }
        Ok(())
    }

    /// One node's style, by where it is in the walk.
    pub(super) fn style_of(&self, position: usize) -> Style {
        self.tree
            .node(self.order[position])
            .map_or(Style::DEFAULT, |node| node.style)
    }

    /// `total`, split between the children by a share each, exactly.
    ///
    /// `whole` is what a full claim is: `Factor16::MAX` when the shares are
    /// read against the axis, and the sum of the shares when they are read
    /// against each other. The claims accumulate rather than being divided one
    /// at a time, so the differences between consecutive results sum to
    /// exactly `total` and the remainder lands on the last of them.
    pub(super) fn distribute(
        &self,
        total: I16F16,
        share: impl Fn(&Kid) -> Factor16,
        whole: u32,
        id: NodeId,
        axis: Axis,
    ) -> Result<Vec<I16F16>, TooLarge> {
        let unit = u64::from(Factor16::MAX.to_bits());
        let mut out = Vec::with_capacity(self.kids.len());
        let mut running: u64 = 0;
        let mut previous = I16F16::ZERO;
        for kid in &self.kids {
            let claimed = share(kid);
            if claimed == Factor16::ZERO {
                out.push(I16F16::ZERO);
                continue;
            }
            running += u64::from(claimed.to_bits());
            let claim = (running * unit / u64::from(whole.max(1))).min(unit);
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the min above holds the claim at Factor16::MAX, which is what a u16 holds"
            )]
            let accumulated = scale_exactly(total, Factor16::from_bits(claim as u16))
                .ok_or(TooLarge { node: id, axis })?;
            out.push(accumulated.saturating_sub(previous));
            previous = accumulated;
        }
        Ok(out)
    }

    /// Place the children now that their sizes along the axis are settled.
    pub(super) fn arrange(
        &mut self,
        style: Style,
        inner: Rect,
        gap: I16F16,
        leftover: I16F16,
        clip: u32,
    ) -> Result<(), TooLarge> {
        let horizontal = style.axis.is_horizontal();
        let (main_start, cross_start) = if horizontal {
            (inner.x, inner.y)
        } else {
            (inner.y, inner.x)
        };
        let inner_cross = if horizontal {
            inner.height
        } else {
            inner.width
        };
        let last = self.kids.len() - 1;
        let mut used = I16F16::ZERO;
        for index in 0..self.kids.len() {
            let kid = self.kids[index];
            let child = self.order[kid.at];
            let child_style = self.style_of(kid.at);
            let lead = justify_lead(style.justify, leftover, index, self.kids.len());
            let main = main_start
                .saturating_add(used)
                .saturating_add(lead)
                .saturating_add(kid.margin_start);

            let (cross_length, cross_content, cross_margin, cross_margin_start) = if horizontal {
                (
                    child_style.height,
                    self.measured[kid.at].height,
                    child_style.margin.vertical(self.scale),
                    child_style.margin.top,
                )
            } else {
                (
                    child_style.width,
                    self.measured[kid.at].width,
                    child_style.margin.horizontal(self.scale),
                    child_style.margin.left,
                )
            };
            let cross_axis = if horizontal { Axis::Column } else { Axis::Row };
            let cross_margin = cross_margin.ok_or(TooLarge {
                node: child,
                axis: cross_axis,
            })?;
            let available = inner_cross.saturating_sub(cross_margin).max(I16F16::ZERO);
            let cross_size =
                if matches!(cross_length, Length::Auto) && matches!(style.align, Align::Stretch) {
                    available
                } else {
                    resolve(
                        cross_length,
                        self.scale,
                        available,
                        cross_content,
                        child,
                        cross_axis,
                    )?
                };
            let cross_size = self.bound(child, child_style, cross_size, available, cross_axis)?;
            let slack = available.saturating_sub(cross_size).max(I16F16::ZERO);
            let offset = match style.align {
                Align::Start | Align::Stretch => I16F16::ZERO,
                Align::Centre => split(slack, 1, 2),
                Align::End => slack,
            };
            let cross = cross_start
                .saturating_add(length(cross_margin_start, self.scale, child, cross_axis)?)
                .saturating_add(offset);

            self.rect[kid.at] = if horizontal {
                Rect::new(main, cross, kid.main, cross_size)
            } else {
                Rect::new(cross, main, cross_size, kid.main)
            };
            self.clip[kid.at] = clip;

            used = used
                .saturating_add(kid.margin)
                .saturating_add(kid.main)
                .saturating_add(if index == last { I16F16::ZERO } else { gap });
        }
        Ok(())
    }
}
