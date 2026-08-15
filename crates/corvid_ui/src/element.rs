//! What a game builds every frame, before anything is reconciled.
//!
//! The seam against `build.rs` is the tree: nothing here touches one. An
//! [`Element`] is the whole description a game writes out again per frame, and
//! `build.rs` is what turns two of them into as little work as possible.

use alloc::vec::Vec;
use core::hash::Hash;

use corvid_color::Rgba8;
use corvid_hash::Digest;

use crate::{
    arena::Key,
    build::flatten,
    focus::Signal,
    length::{Edges, Length},
    style::{Align, Axis, Justify, Style},
    widget::Kind,
};

/// What a game builds and hands to [`Tree::reconcile`](crate::Tree::reconcile).
///
/// Allocates, and lives one frame.
///
/// ```
/// use corvid_ui::{Length, button, column, label};
///
/// #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
/// enum Intent {
///     Play,
///     Settings,
/// }
///
/// let menu = column()
///     .gap(Length::REM)
///     .child(label("cradle"))
///     .child(button("play", Intent::Play))
///     .child(button("settings", Intent::Settings));
/// assert_eq!(menu.count(), 6);
/// ```
///
/// Six and not four, because a button is a box with a label in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element<I> {
    /// How its parent recognises it across a rebuild.
    pub key: Key,
    /// What it is.
    pub kind: Kind<I>,
    /// How it is laid out and drawn.
    pub style: Style,
    /// What is under it, in declaration order.
    pub children: Vec<Self>,
}

impl<I> Element<I> {
    /// An element of this kind, styled this way, with no children.
    #[must_use]
    pub const fn new(kind: Kind<I>, style: Style) -> Self {
        Self {
            key: Key::Index(0),
            kind,
            style,
            children: Vec::new(),
        }
    }

    /// The same element, recognised by a name rather than by its position.
    ///
    /// A row keyed on a tower's id keeps its focus and its layout when the row
    /// above it is removed; a row keyed on its position does not, because its
    /// position is what changed.
    #[must_use]
    pub const fn keyed(mut self, key: u64) -> Self {
        self.key = Key::Named(key);
        self
    }

    /// The same element, with one more child.
    #[must_use]
    pub fn child(mut self, child: impl Into<Self>) -> Self {
        let mut child = child.into();
        if let Key::Index(_) = child.key {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the index is a position among siblings; a parent with four billion children is not a case this crate is asked to answer"
            )]
            {
                child.key = Key::Index(self.children.len() as u32);
            }
        }
        self.children.push(child);
        self
    }

    /// The same element, with all of these children.
    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = Self>) -> Self {
        for child in children {
            self = self.child(child);
        }
        self
    }

    /// How many elements this is, counting itself.
    #[must_use]
    pub fn count(&self) -> usize {
        let mut counted = 0;
        let mut stack: Vec<&Self> = alloc::vec![self];
        while let Some(element) = stack.pop() {
            counted += 1;
            stack.extend(element.children.iter());
        }
        counted
    }

    /// The digest of this element and everything under it.
    ///
    /// Computed on the way down with an explicit stack, so a deep tree costs
    /// heap rather than stack, and compared once on the way in -- computing it
    /// inside `reconcile` would mean walking the subtree to decide whether to
    /// walk the subtree.
    #[must_use]
    pub fn subtree_digest(&self) -> Digest
    where
        I: Hash,
    {
        flatten(self)
            .first()
            .map_or(Digest::ZERO, |flat| flat.subtree)
    }

    /// The same element, styled this way.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The same element, with this much between its children.
    #[must_use]
    pub const fn gap(mut self, gap: Length) -> Self {
        self.style.gap = gap;
        self
    }

    /// The same element, with this padding.
    #[must_use]
    pub const fn padding(mut self, padding: Edges) -> Self {
        self.style.padding = padding;
        self
    }

    /// The same element, with this margin.
    #[must_use]
    pub const fn margin(mut self, margin: Edges) -> Self {
        self.style.margin = margin;
        self
    }

    /// The same element, this wide.
    #[must_use]
    pub const fn width(mut self, width: Length) -> Self {
        self.style.width = width;
        self
    }

    /// The same element, this tall.
    #[must_use]
    pub const fn height(mut self, height: Length) -> Self {
        self.style.height = height;
        self
    }

    /// The same element, filled with this.
    #[must_use]
    pub const fn background(mut self, colour: Rgba8) -> Self {
        self.style.background = colour;
        self
    }

    /// The same element, with text drawn in this.
    #[must_use]
    pub const fn foreground(mut self, colour: Rgba8) -> Self {
        self.style.foreground = colour;
        self
    }

    /// The same element, laying its children out along this axis.
    #[must_use]
    pub const fn axis(mut self, axis: Axis) -> Self {
        self.style.axis = axis;
        self
    }

    /// The same element, putting the leftover space here.
    #[must_use]
    pub const fn justify(mut self, justify: Justify) -> Self {
        self.style.justify = justify;
        self
    }

    /// The same element, aligning its children this way across the axis.
    #[must_use]
    pub const fn align(mut self, align: Align) -> Self {
        self.style.align = align;
        self
    }

    /// The same element, focusable or not.
    #[must_use]
    pub const fn focusable(mut self, focusable: bool) -> Self {
        self.style.focusable = focusable;
        self
    }

    /// The same element, raising `intent` on `signal`.
    ///
    /// A box that raises an intent is a button, whatever it was before. Not
    /// `const`, and it is the one builder here that is not: replacing the kind
    /// drops the kind that was there, and a generic `I` might have a
    /// destructor, which is not something a `const` evaluation may run.
    #[must_use]
    pub fn on(mut self, signal: Signal, intent: I) -> Self {
        self.kind = Kind::Button { on: signal, intent };
        self.style.focusable = true;
        self
    }
}

/// Hashed as its subtree digest, which is the same walk without the recursion.
impl<I: Hash> Hash for Element<I> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.subtree_digest().to_u64());
    }
}
