//! What a solved layout is: rectangles, glyph runs and a clip stack, in draw
//! order, with no device named anywhere.

use alloc::vec::Vec;

use crate::{arena::NodeId, text::GlyphId};
use corvid_color::Rgba8;
use corvid_fixed::I16F16;

/// A point in layout space: physical pixels, right and down from the top left
/// of the viewport.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bytemuck", derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct Position {
    /// Right of the viewport's left edge.
    pub x: I16F16,
    /// Below the viewport's top edge.
    pub y: I16F16,
}

impl Position {
    /// The top left of the viewport.
    pub const ORIGIN: Self = Self::new(I16F16::ZERO, I16F16::ZERO);

    /// A position from its two coordinates.
    #[must_use]
    pub const fn new(x: I16F16, y: I16F16) -> Self {
        Self { x, y }
    }
}

impl From<(I16F16, I16F16)> for Position {
    fn from((x, y): (I16F16, I16F16)) -> Self {
        Self::new(x, y)
    }
}

impl From<Position> for (I16F16, I16F16) {
    fn from(position: Position) -> Self {
        (position.x, position.y)
    }
}

/// A resolved rectangle, in physical pixels from the top left.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bytemuck", derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct Rect {
    /// The left edge.
    pub x: I16F16,
    /// The top edge.
    pub y: I16F16,
    /// How far right it reaches.
    pub width: I16F16,
    /// How far down it reaches.
    pub height: I16F16,
}

impl Rect {
    /// Nothing, at the origin.
    pub const ZERO: Self = Self::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO, I16F16::ZERO);

    /// A rectangle from its corner and its size.
    #[must_use]
    pub const fn new(x: I16F16, y: I16F16, width: I16F16, height: I16F16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// A rectangle of this size at the origin.
    #[must_use]
    pub const fn of(width: I16F16, height: I16F16) -> Self {
        Self::new(I16F16::ZERO, I16F16::ZERO, width, height)
    }

    /// The right edge.
    #[must_use]
    pub const fn right(self) -> I16F16 {
        self.x.saturating_add(self.width)
    }

    /// The bottom edge.
    #[must_use]
    pub const fn bottom(self) -> I16F16 {
        self.y.saturating_add(self.height)
    }

    /// Whether a position is inside, counting the top and left edges and not
    /// the bottom and right — so two rectangles that share an edge do not both
    /// contain a point on it.
    ///
    /// ```
    /// use corvid_fixed::I16F16;
    /// use corvid_ui::{Position, Rect};
    ///
    /// let ten = I16F16::from_f64(10.0);
    /// let rect = Rect::of(ten, ten);
    /// assert!(rect.contains(Position::ORIGIN));
    /// assert!(!rect.contains(Position::new(ten, I16F16::ZERO)));
    /// ```
    #[must_use]
    pub const fn contains(self, at: Position) -> bool {
        within(at.x, self.x, self.right()) && within(at.y, self.y, self.bottom())
    }

    /// The middle.
    #[must_use]
    pub const fn centre(self) -> Position {
        Position::new(
            self.x
                .saturating_add(I16F16::from_bits(self.width.to_bits() / 2)),
            self.y
                .saturating_add(I16F16::from_bits(self.height.to_bits() / 2)),
        )
    }

    /// The largest rectangle inside both, or [`ZERO`](Self::ZERO) when they do
    /// not overlap. What a nested clip is.
    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        let x = if self.x.to_bits() > other.x.to_bits() {
            self.x
        } else {
            other.x
        };
        let y = if self.y.to_bits() > other.y.to_bits() {
            self.y
        } else {
            other.y
        };
        let right = if self.right().to_bits() < other.right().to_bits() {
            self.right()
        } else {
            other.right()
        };
        let bottom = if self.bottom().to_bits() < other.bottom().to_bits() {
            self.bottom()
        } else {
            other.bottom()
        };
        if right.to_bits() <= x.to_bits() || bottom.to_bits() <= y.to_bits() {
            Self::ZERO
        } else {
            Self::new(x, y, right.saturating_sub(x), bottom.saturating_sub(y))
        }
    }
}

/// Whether `value` is at or after `start` and before `end`.
const fn within(value: I16F16, start: I16F16, end: I16F16) -> bool {
    value.to_bits() >= start.to_bits() && value.to_bits() < end.to_bits()
}

/// Where one node of the tree ended up.
///
/// The list of these is what focus navigation searches: it is in tree order,
/// which is what `Compass::Next` means, and it carries the rectangle, which is
/// what the four compass directions mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaintedNode {
    /// Which node.
    pub node: NodeId,
    /// Its border box.
    pub rect: Rect,
    /// Whether the focus may land on it.
    pub focusable: bool,
    /// Which entry of [`Painted::clips`] it is scissored to.
    pub clip: u32,
}

/// One rounded, bordered rectangle.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bytemuck", derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct PaintedRect {
    /// Where it is.
    pub rect: Rect,
    /// What fills it.
    pub fill: Rgba8,
    /// What outlines it.
    pub border: Rgba8,
    /// How thick that outline is.
    pub border_width: I16F16,
    /// The corner radius.
    pub corner: I16F16,
    /// Which entry of [`Painted::clips`] it is scissored to.
    pub clip: u32,
}

/// One glyph, placed.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bytemuck", derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct PaintedGlyph {
    /// The pen position, on the baseline.
    pub at: Position,
    /// Which glyph.
    pub glyph: GlyphId,
    /// The em size it is drawn at.
    pub size: I16F16,
    /// What it is drawn in.
    pub tint: Rgba8,
    /// Which entry of [`Painted::clips`] it is scissored to.
    pub clip: u32,
}

/// How much work the solver did, which is what makes its cost checkable.
///
/// Two passes and no more, so both numbers are the node count and a layout is
/// O(nodes) rather than something that iterates to a fixed point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Visits {
    /// Nodes measured, bottom up.
    pub measured: u32,
    /// Nodes placed, top down.
    pub placed: u32,
}

/// Everything a renderer needs and nothing it does not.
///
/// Integers throughout, so the same tree solved twice gives the same bytes and
/// a UI regression is a golden diff rather than a screenshot someone eyeballs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Painted {
    /// Every node, in tree order, with where it landed.
    pub nodes: Vec<PaintedNode>,
    /// The rectangles, in draw order.
    pub rects: Vec<PaintedRect>,
    /// The glyphs, in draw order.
    pub glyphs: Vec<PaintedGlyph>,
    /// The scissor rectangles the two lists above index. Entry zero is the
    /// viewport, so a `clip` of zero is no clipping at all.
    pub clips: Vec<Rect>,
    /// The viewport this was solved for.
    pub size: Rect,
    /// What the solver did to produce it.
    pub visits: Visits,
}

impl Painted {
    /// Where a node landed.
    #[must_use]
    pub fn rect_of(&self, node: NodeId) -> Option<Rect> {
        self.nodes
            .iter()
            .find(|painted| painted.node == node)
            .map(|painted| painted.rect)
    }

    /// The focusable node under a position, or nothing when there is none.
    ///
    /// The last one in tree order wins, which is the one drawn on top.
    #[must_use]
    pub fn focusable_at(&self, at: Position) -> Option<NodeId> {
        self.nodes
            .iter()
            .rev()
            .find(|painted| painted.focusable && painted.rect.contains(at))
            .map(|painted| painted.node)
    }

    /// Every focusable node, in tree order.
    pub fn focusable(&self) -> impl Iterator<Item = &PaintedNode> + '_ {
        self.nodes.iter().filter(|painted| painted.focusable)
    }
}
