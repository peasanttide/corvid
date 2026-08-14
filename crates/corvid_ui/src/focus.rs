//! One cursor and two primitives, so four devices drive one tree.
//!
//! Every input device reduces to exactly two things: a step in a direction,
//! and a point in layout space. A gamepad produces the first. A mouse, a
//! finger, and an XR ray that [`Painted::panel_to_layout`] has already
//! intersected all produce the second -- so the headset path and the mouse path
//! are the same code below the intersection, and there is no XR-specific
//! branch anywhere in this crate.

use alloc::vec::Vec;

use crate::{
    arena::{NodeId, Tree},
    paint::{Painted, Position},
    widget::Kind,
};
use corvid_fixed::{Factor16, I16F16};
use corvid_shape::{Cast as _, Plane, Ray};
use corvid_transform::Transform;

/// Which way a navigation step goes.
///
/// The four compass directions are spatial and are what a stick wants.
/// [`Next`](Compass::Next) and [`Previous`](Compass::Previous) are tree order
/// and are what a Tab key and a form want. Conflating the two is why some
/// menus skip a button when the layout wraps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Compass {
    /// Towards the top of the screen.
    Up,
    /// Towards the bottom.
    Down,
    /// Towards the left.
    Left,
    /// Towards the right.
    Right,
    /// The next focusable in the order the game wrote them, wrapping.
    Next,
    /// The previous one, wrapping.
    Previous,
}

impl Compass {
    /// Whether this is one of the four spatial directions.
    #[must_use]
    pub const fn is_spatial(self) -> bool {
        !matches!(self, Self::Next | Self::Previous)
    }
}

/// What a widget raises.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Signal {
    /// Pressed, clicked, or tapped.
    Activate,
    /// Its value moved.
    Change,
    /// The focus arrived.
    Focus,
    /// The focus left.
    Blur,
    /// Backed out of.
    Cancel,
}

/// The focus, which survives a reconcile whenever the node's
/// [`Key`](crate::Key) does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Focus {
    /// Which node, or [`NodeId::NONE`].
    pub node: NodeId,
    /// Whether it should be shown. A focus a player moved with a stick is
    /// visible; a focus a mouse put somewhere is shown by the cursor already.
    pub visible: bool,
}

impl Focus {
    /// No focus.
    pub const NOWHERE: Self = Self {
        node: NodeId::NONE,
        visible: false,
    };

    /// The focus on a node, shown.
    #[must_use]
    pub const fn shown(node: NodeId) -> Self {
        Self {
            node,
            visible: true,
        }
    }

    /// Whether anything holds it.
    #[must_use]
    pub const fn is_somewhere(self) -> bool {
        self.node.is_some()
    }
}

/// One intent, raised by one node, on one signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Raised<I> {
    /// Which node raised it.
    pub node: NodeId,
    /// What happened.
    pub signal: Signal,
    /// What the game asked to be told.
    pub intent: I,
}

impl<I: Copy + Eq + core::hash::Hash> Tree<I> {
    /// Move the focus one step.
    ///
    /// The four compass directions are a search over the resolved rectangles:
    /// among the focusable nodes whose centre lies in the half-plane the
    /// compass names, the one minimising `along + 2 * across`, where `along`
    /// is the distance between the centres in the direction of travel and
    /// `across` is the gap between the two rectangles perpendicular to it --
    /// zero while they overlap. Weighting the perpendicular twice is what
    /// stops a stick-right from skipping a column.
    ///
    /// A step off the edge leaves the focus where it was.
    /// [`Next`](Compass::Next) and [`Previous`](Compass::Previous) wrap.
    pub fn navigate(&mut self, painted: &Painted, step: Compass) -> Focus {
        let focus = match step {
            Compass::Next => order_step(painted, self.focus().node, true),
            Compass::Previous => order_step(painted, self.focus().node, false),
            direction => spatial_step(painted, self.focus().node, direction),
        };
        if let Some(node) = focus {
            self.set_focus(Focus::shown(node));
        }
        self.focus()
    }

    /// Put the focus wherever this layout-space position lands.
    ///
    /// A mouse, a finger, and an XR ray that has already been intersected all
    /// arrive here. A position over nothing focusable blurs, rather than
    /// leaving a stale focus behind for the next `Activate` to fire at.
    pub fn point(&mut self, painted: &Painted, at: Position) -> Focus {
        let focus = painted
            .focusable_at(at)
            .map_or(Focus::NOWHERE, |node| Focus {
                node,
                visible: false,
            });
        self.set_focus(focus);
        focus
    }

    /// Raise a signal at the focused node, and collect what it produced.
    ///
    /// A button raises its intent when the signal is the one it was built
    /// with. A toggle flips and raises [`Signal::Change`], because what a game
    /// wants to be told about a toggle is that it changed rather than that it
    /// was pressed.
    pub fn signal(&mut self, signal: Signal, out: &mut Vec<Raised<I>>) {
        let node = self.focus().node;
        let Some(kind) = self.node(node).map(|it| it.kind) else {
            return;
        };
        match kind {
            Kind::Button { on, intent } if on == signal => out.push(Raised {
                node,
                signal,
                intent,
            }),
            Kind::Toggle { on, intent } if signal == Signal::Activate => {
                if let Some(it) = self.node_mut(node) {
                    it.kind = Kind::Toggle { on: !on, intent };
                }
                out.push(Raised {
                    node,
                    signal: Signal::Change,
                    intent,
                });
            }
            _ => {}
        }
    }

    /// Drag the focused slider to a value.
    ///
    /// The value is a [`Factor16`], so "clamped at both ends" is a property of
    /// the type rather than a line of code that could be forgotten. A drag
    /// that does not move the slider raises nothing.
    pub fn drag(&mut self, value: Factor16, out: &mut Vec<Raised<I>>) {
        let node = self.focus().node;
        let Some(Kind::Slider {
            value: at,
            step,
            intent,
        }) = self.node(node).map(|it| it.kind)
        else {
            return;
        };
        if at == value {
            return;
        }
        if let Some(it) = self.node_mut(node) {
            it.kind = Kind::Slider {
                value,
                step,
                intent,
            };
        }
        out.push(Raised {
            node,
            signal: Signal::Change,
            intent,
        });
    }

    /// Nudge the focused slider one step, up or down.
    ///
    /// A step of zero is a slider that cannot be nudged, which is what a
    /// slider a game wants dragged and not stepped says.
    pub fn nudge(&mut self, up: bool, out: &mut Vec<Raised<I>>) {
        let node = self.focus().node;
        let Some(Kind::Slider { value, step, .. }) = self.node(node).map(|it| it.kind) else {
            return;
        };
        if step == Factor16::ZERO {
            return;
        }
        let moved = if up {
            value.saturating_add(step)
        } else {
            value.saturating_sub(step)
        };
        self.drag(moved, out);
    }

    /// Put the focus on a node, whether or not anything pointed at it.
    ///
    /// What a game calls to open a menu with its first button already
    /// selected. A node that is not focusable is refused, and the focus stays
    /// where it was.
    pub fn focus_on(&mut self, node: NodeId) -> Focus {
        if self.node(node).is_some_and(|it| it.style.focusable) {
            self.set_focus(Focus::shown(node));
        }
        self.focus()
    }
}

/// The next or previous focusable in tree order, wrapping.
fn order_step(painted: &Painted, from: NodeId, forwards: bool) -> Option<NodeId> {
    let nodes: Vec<NodeId> = painted.focusable().map(|it| it.node).collect();
    if nodes.is_empty() {
        return None;
    }
    let at = nodes.iter().position(|node| *node == from);
    let next = match (at, forwards) {
        (None, true) => 0,
        (None, false) => nodes.len() - 1,
        (Some(at), true) => (at + 1) % nodes.len(),
        (Some(at), false) => (at + nodes.len() - 1) % nodes.len(),
    };
    nodes.get(next).copied()
}

/// The nearest focusable in the half-plane the compass names.
fn spatial_step(painted: &Painted, from: NodeId, direction: Compass) -> Option<NodeId> {
    let Some(rect) = painted.rect_of(from) else {
        return painted.focusable().next().map(|it| it.node);
    };
    let here = rect.centre();
    let mut best: Option<(i64, NodeId)> = None;
    for candidate in painted.focusable() {
        if candidate.node == from {
            continue;
        }
        let there = candidate.rect.centre();
        let dx = i64::from(there.x.to_bits()) - i64::from(here.x.to_bits());
        let dy = i64::from(there.y.to_bits()) - i64::from(here.y.to_bits());
        let vertical = gap(
            (rect.y, rect.bottom()),
            (candidate.rect.y, candidate.rect.bottom()),
        );
        let horizontal = gap(
            (rect.x, rect.right()),
            (candidate.rect.x, candidate.rect.right()),
        );
        let (along, across) = match direction {
            Compass::Right => (dx, vertical),
            Compass::Left => (-dx, vertical),
            Compass::Down => (dy, horizontal),
            Compass::Up => (-dy, horizontal),
            Compass::Next | Compass::Previous => continue,
        };
        if along <= 0 {
            continue;
        }
        let score = along + 2 * across;
        if best.is_none_or(|(seen, _)| score < seen) {
            best = Some((score, candidate.node));
        }
    }
    best.map(|(_, node)| node)
}

/// How far apart two spans are, and zero while they overlap.
///
/// The perpendicular term is a gap between the rectangles rather than a
/// distance between their centres, because a menu column of buttons of
/// different widths has its centres at different places and its edges lined
/// up. Scored on centres, a stick-down out of a wide button lands on whichever
/// narrow one happens to be centred nearest -- which is the button below the
/// next one, and looks like the menu skipping.
fn gap(here: (I16F16, I16F16), there: (I16F16, I16F16)) -> i64 {
    let (near, far) = (i64::from(here.0.to_bits()), i64::from(here.1.to_bits()));
    let (start, end) = (i64::from(there.0.to_bits()), i64::from(there.1.to_bits()));
    (near - end).max(start - far).max(0)
}

impl Painted {
    /// Where on this panel a world-space ray lands, in layout space.
    ///
    /// The XR path, and the only thing in this crate that knows XR exists. The
    /// panel is a quad `metres` wide, centred at `pose` and facing
    /// [`Transform::forward`], as tall as this layout's aspect ratio makes it;
    /// the returned position is in the same coordinates [`Tree::point`] takes.
    /// A ray that misses the quad, a panel of no width, and a layout of no
    /// size all answer [`None`].
    ///
    /// `pose` is a [`Transform`] rather than a `FineTransform` because a
    /// [`Ray`] is cast in `GlobalPoint` and the two have to be the same tier
    /// for the intersection to be exact. The
    /// resolution that leaves is the ray's own: 3.9 mm, which on a metre-wide
    /// panel a thousand pixels across is a four-pixel step, and is finer than
    /// the hand holding the controller.
    #[must_use]
    pub fn panel_to_layout(&self, pose: Transform, metres: I16F16, ray: Ray) -> Option<Position> {
        if metres.to_bits() <= 0
            || self.size.width.to_bits() <= 0
            || self.size.height.to_bits() <= 0
        {
            return None;
        }
        let hit = Plane::through(pose.position(), pose.forward()).cast(ray)?;
        let offset = hit.point.sub(pose.position());
        let across = offset.project(pose.right());
        let up = offset.project(pose.up());

        // Half the panel, in the ray's own Q8 metres.
        let half_width = i128::from(metres.to_i24f8().to_bits()) / 2;
        if half_width <= 0 {
            return None;
        }
        let half_height = half_width * i128::from(self.size.height.to_bits())
            / i128::from(self.size.width.to_bits());
        if half_height <= 0 {
            return None;
        }
        let (across, up) = (i128::from(across.to_bits()), i128::from(up.to_bits()));
        if across.abs() > half_width || up.abs() > half_height {
            return None;
        }

        let x = i128::from(self.size.width.to_bits()) * (across + half_width) / (2 * half_width);
        let y = i128::from(self.size.height.to_bits()) * (half_height - up) / (2 * half_height);
        Some(Position::new(
            self.size.x.saturating_add(narrow(x)),
            self.size.y.saturating_add(narrow(y)),
        ))
    }
}

/// A wide intermediate as the length it denotes, clamping rather than wrapping.
///
/// The saturation is `corvid_bits`'; what is left here is which fixed-point
/// type the bits are read as, which is this crate's to say.
const fn narrow(value: i128) -> I16F16 {
    I16F16::from_bits(corvid_bits::narrow_i128(value))
}
