//! The flex solver: two passes over the tree, and no more.
//!
//! Pass one walks bottom-up measuring intrinsic content size — a label asks
//! [`Metrics`], a container sums its children along its axis and takes the
//! maximum across it. Pass two walks top-down distributing free space to
//! `Fraction` children and assigning positions. Nothing iterates to a fixed
//! point, so a layout is O(nodes) and its cost is knowable from the node
//! count alone.

use alloc::vec::Vec;
use core::hash::Hash;

use crate::{
    arena::{NodeId, Tree},
    length::{Length, Scale, edge, scale_exactly, split},
    paint::{Painted, PaintedGlyph, PaintedNode, PaintedRect, Position, Rect, Visits},
    style::{Align, Axis, Justify, Style},
    text::{Line, Metrics},
    widget::Kind,
};
use corvid_fixed::{Factor16, I16F16};

/// A resolved length ran past what `I16F16` can hold.
///
/// The saturation `I16F16` would do instead is silent, and a layout that
/// silently clamped is a menu that is subtly wrong on one machine. The node
/// named is the one whose arithmetic ran past the type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TooLarge {
    /// Whose arithmetic it was.
    pub node: NodeId,
    /// Which direction it ran in. [`Axis::Row`] is the horizontal one.
    pub axis: Axis,
}

impl core::fmt::Display for TooLarge {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "node {} is larger along {:?} than I16F16 holds",
            self.node.0, self.axis
        )
    }
}

impl core::error::Error for TooLarge {}

/// How wide a slider is when nothing says otherwise, in multiples of its text
/// size.
const SLIDER_WIDTH: I16F16 = I16F16::from_f64(8.0);

/// How wide a toggle is when nothing says otherwise, in multiples of its text
/// size.
const TOGGLE_WIDTH: I16F16 = I16F16::from_f64(2.0);

/// Solve the tree's layout into paint data.
///
/// ```
/// use corvid_fixed::I16F16;
/// use corvid_ui::{Length, Monospace, Rect, Scale, Tree, column, label, solve};
///
/// let mut tree = Tree::<()>::new();
/// tree.reconcile(column().child(label("hello")).child(label("world")));
///
/// let viewport = Rect::of(I16F16::from_f64(320.0), I16F16::from_f64(200.0));
/// let painted = solve(&tree, &Monospace::DEFAULT, Scale::DEFAULT, viewport)?;
///
/// // Two passes over three nodes.
/// assert_eq!(painted.visits.measured, 3);
/// assert_eq!(painted.visits.placed, 3);
/// // Sixteen pixels to the rem, half of that to a character: five characters.
/// assert_eq!(painted.nodes[1].rect.width, I16F16::from_f64(40.0));
/// # Ok::<(), corvid_ui::TooLarge>(())
/// ```
///
/// # Errors
///
/// [`TooLarge`] when a resolved length ran past what `I16F16` holds, naming
/// the node it happened at.
pub fn solve<I: Copy + Eq + Hash>(
    tree: &Tree<I>,
    metrics: &dyn Metrics,
    scale: Scale,
    viewport: Rect,
) -> Result<Painted, TooLarge> {
    let mut painted = Painted {
        size: viewport,
        clips: alloc::vec![viewport],
        ..Painted::default()
    };
    if tree.root().is_none() {
        return Ok(painted);
    }

    let order: Vec<NodeId> = tree.preorder().collect();
    let mut at = alloc::vec![usize::MAX; tree.slots()];
    for (position, id) in order.iter().enumerate() {
        if let Some(slot) = id.index() {
            at[slot] = position;
        }
    }
    let count = order.len();
    let mut solver = Solver {
        tree,
        metrics,
        scale,
        order,
        at,
        measured: alloc::vec![Measured::ZERO; count],
        rect: alloc::vec![Rect::ZERO; count],
        clip: alloc::vec![0; count],
        kids: Vec::new(),
    };

    solver.measure()?;
    solver.place(viewport, &mut painted)?;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a UI with four billion nodes is not a case this crate is asked to answer, and a NodeId is a u32 index long before this count is"
    )]
    {
        painted.visits = Visits {
            measured: count as u32,
            placed: count as u32,
        };
    }
    Ok(painted)
}

/// What pass one worked out about one node.
#[derive(Clone, Copy, Debug)]
struct Measured {
    /// The em size its text is set at.
    font: I16F16,
    /// Its intrinsic border box: content, padding and border, before its own
    /// width and height are applied.
    width: I16F16,
    /// The same, downwards.
    height: I16F16,
    /// Its intrinsic outer box: the above with its width, height, bounds and
    /// margin applied, which is what its parent sums.
    outer_width: I16F16,
    /// The same, downwards.
    outer_height: I16F16,
}

impl Measured {
    const ZERO: Self = Self {
        font: I16F16::ZERO,
        width: I16F16::ZERO,
        height: I16F16::ZERO,
        outer_width: I16F16::ZERO,
        outer_height: I16F16::ZERO,
    };
}

/// One child, as the pass that places them reads it.
#[derive(Clone, Copy, Debug)]
struct Kid {
    /// Where it is in `order`.
    at: usize,
    /// Its size along the parent's axis.
    main: I16F16,
    /// Its margins along that axis, summed.
    margin: I16F16,
    /// The near one of those two.
    margin_start: I16F16,
    /// The share of the free space it asked for, or zero.
    share: Factor16,
    /// Whether its bounds cut its share down, in which case it does not take
    /// part in the one redistribution round.
    clamped: bool,
}

/// The two passes, and everything they share.
struct Solver<'a, I> {
    tree: &'a Tree<I>,
    metrics: &'a dyn Metrics,
    scale: Scale,
    order: Vec<NodeId>,
    at: Vec<usize>,
    measured: Vec<Measured>,
    rect: Vec<Rect>,
    clip: Vec<u32>,
    kids: Vec<Kid>,
}

impl<I: Copy + Eq + Hash> Solver<'_, I> {
    /// Where a node is in the walk.
    fn position_of(&self, id: NodeId) -> Option<usize> {
        self.at
            .get(id.index()?)
            .copied()
            .filter(|position| *position != usize::MAX)
    }

    /// Pass one: every node's intrinsic size, children before parents.
    fn measure(&mut self) -> Result<(), TooLarge> {
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
    fn outer(
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
    fn bound(
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

    /// Pass two: every node's rectangle, parents before children, painting
    /// each as it goes.
    fn place(&mut self, viewport: Rect, painted: &mut Painted) -> Result<(), TooLarge> {
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
    fn content_box(&self, position: usize, style: Style) -> Result<Rect, TooLarge> {
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

    /// Collect a node's children, with their fixed sizes already resolved.
    fn gather(&mut self, id: NodeId, style: Style) -> Result<(), TooLarge> {
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
    fn taken(&self, gaps: I16F16, axis: Axis) -> Result<I16F16, TooLarge> {
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
    fn share(&mut self, free: I16F16, id: NodeId, axis: Axis) -> Result<(), TooLarge> {
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
    fn style_of(&self, position: usize) -> Style {
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
    fn distribute(
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
    fn arrange(
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

    /// Everything one node contributes to the paint data.
    fn paint(&self, position: usize, painted: &mut Painted) {
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

/// The main and cross axis of a container, in that order.
const fn axes(axis: Axis) -> (Axis, Axis) {
    (axis, axis.across())
}

/// How much space goes before the child at `index`.
const fn justify_lead(justify: Justify, leftover: I16F16, index: usize, count: usize) -> I16F16 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a container with four billion children is not a case this crate is asked to answer"
    )]
    let (index, count) = (index as u32, count as u32);
    match justify {
        Justify::Start => I16F16::ZERO,
        Justify::End => leftover,
        Justify::Centre => split(leftover, 1, 2),
        Justify::Between => split(leftover, index, count.saturating_sub(1)),
        Justify::Around => split(leftover, 2 * index + 1, 2 * count),
    }
}

/// A resolved length, or the node that could not have one.
fn resolve(
    length: Length,
    scale: Scale,
    free: I16F16,
    content: I16F16,
    node: NodeId,
    axis: Axis,
) -> Result<I16F16, TooLarge> {
    length
        .checked_resolve(scale, free, content)
        .ok_or(TooLarge { node, axis })
}

/// A length resolved against nothing to share and nothing to measure.
fn length(value: Length, scale: Scale, node: NodeId, axis: Axis) -> Result<I16F16, TooLarge> {
    edge(value, scale).ok_or(TooLarge { node, axis })
}

/// `left + right`, or the node that could not hold it.
fn add(left: I16F16, right: I16F16, node: NodeId, axis: Axis) -> Result<I16F16, TooLarge> {
    left.checked_add(right).ok_or(TooLarge { node, axis })
}

/// The same, where the left operand may already have failed to resolve.
fn sum(left: Option<I16F16>, right: I16F16, node: NodeId, axis: Axis) -> Result<I16F16, TooLarge> {
    add(left.ok_or(TooLarge { node, axis })?, right, node, axis)
}

/// `value * count`, or the node that could not hold it.
fn times(value: I16F16, count: usize, node: NodeId, axis: Axis) -> Result<I16F16, TooLarge> {
    value
        .checked_mul(count_as(count))
        .ok_or(TooLarge { node, axis })
}

/// A count as the whole number `I16F16` holds, saturating past 32 767.
fn count_as(count: usize) -> I16F16 {
    I16F16::from_bits(
        i32::try_from(count)
            .unwrap_or(i32::MAX)
            .saturating_mul(1 << 16),
    )
}

/// A style's width or height when it is a fixed number of pixels, and nothing
/// when it is a share or as large as its content.
const fn fixed(value: Length, scale: Scale) -> Option<I16F16> {
    match value {
        Length::Rem(_) | Length::Px(_) => value.checked_resolve(scale, I16F16::ZERO, I16F16::ZERO),
        Length::Fraction(_) | Length::Auto => None,
    }
}

/// Where a wrapped label breaks, as byte ranges into its text.
///
/// Greedy and on spaces alone: the first word goes on the line, and each
/// following word joins it while it fits. A word wider than the whole line
/// overflows rather than being cut, because a word cut in half is harder to
/// read than a word that runs over.
fn wrapped(
    text: &Line,
    metrics: &dyn Metrics,
    font: I16F16,
    available: I16F16,
) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    let space = metrics.advance(metrics.glyph(' '), font);
    let mut start = 0;
    let mut end = 0;
    let mut width = I16F16::ZERO;
    let mut offset = 0;
    for word in text.as_str().split(' ') {
        let measured = word.chars().fold(I16F16::ZERO, |total, character| {
            total.saturating_add(metrics.advance(metrics.glyph(character), font))
        });
        let joined = if end > start {
            space.saturating_add(measured)
        } else {
            measured
        };
        if end > start && width.saturating_add(joined) > available {
            lines.push((start, end));
            start = offset;
            end = offset + word.len();
            width = measured;
        } else {
            width = width.saturating_add(joined);
            end = offset + word.len();
        }
        offset += word.len() + 1;
    }
    if end > start || lines.is_empty() {
        lines.push((start, end));
    }
    lines
}
