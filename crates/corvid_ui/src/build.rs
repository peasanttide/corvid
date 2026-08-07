//! What a game builds every frame, and the reconcile that turns it into as
//! little work as possible.
//!
//! Building is immediate — a game writes the whole tree out again — and
//! *reconciliation* is what makes the store retained. Each element carries a
//! digest of its own properties and a digest of its subtree; a subtree whose
//! digest is unchanged is kept whole, including its resolved layout and its
//! focus.

use alloc::vec::Vec;
use core::hash::Hash;

use corvid_color::Rgba8;
use corvid_hash::{Digest, Hasher};

use crate::{
    arena::{Key, Node, NodeId, Rebuilt, Tree},
    focus::{Focus, Signal},
    length::{Edges, Length},
    style::{Align, Axis, Justify, Style},
    widget::Kind,
};

/// What a game builds and hands to [`Tree::reconcile`].
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
    /// heap rather than stack, and compared once on the way in — computing it
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

/// One element, flattened: its two digests, and how many elements its subtree
/// is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Flat {
    /// This element's own properties, hashed.
    pub own: Digest,
    /// This element and everything under it.
    pub subtree: Digest,
    /// How many entries this subtree occupies, counting this one — so the
    /// entry after a whole subtree is at `index + count`.
    pub count: usize,
    /// How many children it has.
    pub children: usize,
}

/// Every element, in pre-order, with both digests already computed.
///
/// Two loops and no recursion. The first walks down with an explicit stack;
/// the second runs backwards over what it collected, which visits every child
/// before its parent because a pre-order puts a parent first.
pub(crate) fn flatten<I: Hash>(root: &Element<I>) -> Vec<Flat> {
    let mut pre: Vec<&Element<I>> = Vec::new();
    let mut stack: Vec<&Element<I>> = alloc::vec![root];
    while let Some(element) = stack.pop() {
        pre.push(element);
        for child in element.children.iter().rev() {
            stack.push(child);
        }
    }

    let mut flat = alloc::vec![
        Flat {
            own: Digest::ZERO,
            subtree: Digest::ZERO,
            count: 1,
            children: 0,
        };
        pre.len()
    ];
    for index in (0..pre.len()).rev() {
        let element = pre[index];
        let own = own_digest(element);
        let mut hasher = Hasher::new().absorb(own.to_u64());
        let mut count = 1;
        let mut child = index + 1;
        for _ in 0..element.children.len() {
            let Some(entry) = flat.get(child) else { break };
            hasher = hasher.absorb(entry.subtree.to_u64());
            count += entry.count;
            child += entry.count;
        }
        flat[index] = Flat {
            own,
            subtree: hasher.digest(),
            count,
            children: element.children.len(),
        };
    }
    flat
}

/// One element's own properties, hashed. Its children are not in it.
fn own_digest<I: Hash>(element: &Element<I>) -> Digest {
    let mut hasher = Hasher::new();
    element.key.hash(&mut hasher);
    element.kind.hash(&mut hasher);
    element.style.hash(&mut hasher);
    hasher.digest()
}

/// Drop an element and everything under it without recursing.
fn discard<I>(element: Element<I>) {
    let mut stack = alloc::vec![element];
    while let Some(mut top) = stack.pop() {
        stack.append(&mut top.children);
    }
}

/// One node to reconcile: the element, where its digests are, and where it
/// goes.
struct Job<I> {
    element: Element<I>,
    flat: usize,
    node: NodeId,
    /// Whether the slot was allocated for this element, in which case there is
    /// nothing there to compare against.
    fresh: bool,
}

impl<I: Copy + Eq + Hash> Tree<I> {
    /// Reconcile an element tree into this one, keeping every subtree whose
    /// digest is unchanged.
    ///
    /// ```
    /// use corvid_ui::{Rebuilt, Tree, column, label};
    ///
    /// fn view() -> corvid_ui::Element<()> {
    ///     column().child(label("score")).child(label("0"))
    /// }
    ///
    /// let mut tree = Tree::new();
    /// tree.reconcile(view());
    /// // The second frame discovers it has nothing to do.
    /// assert_eq!(tree.reconcile(view()), Rebuilt::NOTHING);
    /// ```
    pub fn reconcile(&mut self, root: Element<I>) -> Rebuilt {
        let flat = flatten(&root);
        let mut rebuilt = Rebuilt::NOTHING;

        let mut fresh_root = false;
        if self.root().is_none() {
            let id = self.allocate(blank(root.key, root.kind, root.style));
            self.set_root(id);
            fresh_root = true;
            rebuilt.subtrees += 1;
        }

        let mut jobs = alloc::vec![Job {
            element: root,
            flat: 0,
            node: self.root(),
            fresh: fresh_root,
        }];
        // Reused by every job, so a wide tree does not allocate once a level.
        let mut existing: Vec<(Key, NodeId)> = Vec::new();
        let mut ordered: Vec<NodeId> = Vec::new();

        while let Some(mut job) = jobs.pop() {
            let Some(entry) = flat.get(job.flat).copied() else {
                discard(job.element);
                continue;
            };
            let Some(node) = self.node(job.node) else {
                discard(job.element);
                continue;
            };

            if !job.fresh && node.subtree == entry.subtree {
                discard(job.element);
                continue;
            }

            if job.fresh || node.own != entry.own {
                rebuilt.nodes += 1;
                if let Some(node) = self.node_mut(job.node) {
                    node.key = job.element.key;
                    node.kind = job.element.kind;
                    node.style = job.element.style;
                    node.own = entry.own;
                }
            }
            if let Some(node) = self.node_mut(job.node) {
                node.subtree = entry.subtree;
            }

            existing.clear();
            for child in self.children(job.node) {
                if let Some(node) = self.node(child) {
                    existing.push((node.key, child));
                }
            }

            ordered.clear();
            let mut child_flat = job.flat + 1;
            let children = core::mem::take(&mut job.element.children);
            for child in children {
                let matched = existing
                    .iter()
                    .position(|(key, _)| *key == child.key)
                    .map(|at| existing.remove(at).1);
                let (id, fresh) = if let Some(id) = matched {
                    (id, false)
                } else {
                    rebuilt.subtrees += 1;
                    (
                        self.allocate(blank(child.key, child.kind, child.style)),
                        true,
                    )
                };
                ordered.push(id);
                let count = flat.get(child_flat).map_or(1, |entry| entry.count);
                jobs.push(Job {
                    element: child,
                    flat: child_flat,
                    node: id,
                    fresh,
                });
                child_flat += count;
            }

            while let Some((_, orphan)) = existing.pop() {
                rebuilt.subtrees += 1;
                self.release(orphan);
            }

            let parent = job.node;
            if let Some(node) = self.node_mut(parent) {
                node.first_child = ordered.first().copied().unwrap_or(NodeId::NONE);
            }
            for (at, &id) in ordered.iter().enumerate() {
                let next = ordered.get(at + 1).copied().unwrap_or(NodeId::NONE);
                if let Some(node) = self.node_mut(id) {
                    node.next_sibling = next;
                    node.parent = parent;
                }
            }
        }

        self.settle_focus();
        rebuilt
    }

    /// Forget a focus whose node is gone or is no longer focusable.
    fn settle_focus(&mut self) {
        let focus = self.focus();
        let kept = self
            .node(focus.node)
            .is_some_and(|node| node.style.focusable);
        if !kept {
            self.set_focus(Focus::NOWHERE);
        }
    }
}

/// A node with no digests and no links, which a job is about to fill in.
const fn blank<I>(key: Key, kind: Kind<I>, style: Style) -> Node<I> {
    Node {
        key,
        kind,
        style,
        own: Digest::ZERO,
        subtree: Digest::ZERO,
        first_child: NodeId::NONE,
        next_sibling: NodeId::NONE,
        parent: NodeId::NONE,
    }
}
