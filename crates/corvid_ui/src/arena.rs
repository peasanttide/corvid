//! The retained store: one vector of nodes, and the focus that survives a
//! rebuild.
//!
//! Children are a first-child / next-sibling pair rather than a `Vec`, so a
//! node is a fixed size, adding a child is two writes, and a tree of ten
//! thousand is one allocation.

use alloc::vec::Vec;
use core::hash::{Hash, Hasher};

use corvid_hash::Digest;

use crate::{focus::Focus, style::Style, widget::Kind};

/// Where a node lives in the arena.
///
/// A `u32` index rather than a pointer, so a node is copyable, comparable, and
/// four bytes wherever it is stored.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "bytemuck", derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct NodeId(pub u32);

impl NodeId {
    /// No node.
    ///
    /// The niche a first-child or next-sibling link uses when the list ends,
    /// so a link is a `NodeId` rather than an `Option<NodeId>` and a node stays
    /// the size it is.
    pub const NONE: Self = Self(u32::MAX);

    /// Whether this is [`NONE`](Self::NONE).
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.0 == u32::MAX
    }

    /// Whether this names a slot.
    #[must_use]
    pub const fn is_some(self) -> bool {
        self.0 != u32::MAX
    }

    /// The index into the arena, or nothing for [`NONE`](Self::NONE).
    #[must_use]
    pub const fn index(self) -> Option<usize> {
        if self.is_none() {
            None
        } else {
            Some(self.0 as usize)
        }
    }
}

impl Default for NodeId {
    /// [`NONE`](NodeId::NONE), because a link that has not been set yet points
    /// at nothing rather than at the root.
    fn default() -> Self {
        Self::NONE
    }
}

impl From<u32> for NodeId {
    fn from(index: u32) -> Self {
        Self(index)
    }
}

/// One node's identity within its parent, so a rebuild can tell a moved child
/// from a replaced one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// Its position among its siblings. What a child gets when the game does
    /// not say otherwise.
    Index(u32),
    /// What the game said. A row keyed on a tower's id keeps its focus when
    /// the row above it is removed.
    Named(u64),
}

impl Default for Key {
    fn default() -> Self {
        Self::Index(0)
    }
}

/// One node, as the arena stores it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Node<I> {
    /// How its parent recognises it across a rebuild.
    pub key: Key,
    /// What it is.
    pub kind: Kind<I>,
    /// How it is laid out and drawn.
    pub style: Style,
    /// This node's own properties, hashed. Cheap to compare.
    pub own: Digest,
    /// This node and everything under it. Equal means keep the subtree.
    pub subtree: Digest,
    /// Its first child, or [`NodeId::NONE`].
    pub first_child: NodeId,
    /// The next child of its parent, or [`NodeId::NONE`].
    pub next_sibling: NodeId,
    /// Its parent, or [`NodeId::NONE`] for the root.
    pub parent: NodeId,
}

/// How much of the tree the last reconcile had to touch.
///
/// `nodes` counts the nodes whose own properties were written — a new node, or
/// one whose text or style changed. `subtrees` counts the subtrees created or
/// destroyed whole. An idle frame is `Rebuilt::default()`: a node whose
/// subtree digest is unchanged is not descended into, and a node whose subtree
/// changed but whose own properties did not is relinked without being written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rebuilt {
    /// Nodes whose own properties were written.
    pub nodes: u32,
    /// Subtrees created or destroyed whole.
    pub subtrees: u32,
}

impl Rebuilt {
    /// Nothing was touched.
    pub const NOTHING: Self = Self {
        nodes: 0,
        subtrees: 0,
    };

    /// Whether the last reconcile had nothing to do.
    #[must_use]
    pub const fn is_nothing(self) -> bool {
        self.nodes == 0 && self.subtrees == 0
    }
}

/// The retained store.
///
/// `I` is the game's own intent type — what a button raises when it is
/// activated. It is `Copy + Eq + Hash` because it is part of a node's digest
/// and part of what a rebuild compares.
#[derive(Clone, Debug)]
pub struct Tree<I> {
    /// The arena. A vacant slot is a node that was removed and whose index a
    /// later child will take.
    nodes: Vec<Option<Node<I>>>,
    /// The vacant slots, newest first.
    vacant: Vec<NodeId>,
    /// How many slots are occupied.
    live: usize,
    /// The root, or [`NodeId::NONE`] before the first reconcile.
    root: NodeId,
    /// Where the focus is.
    focus: Focus,
}

impl<I> Tree<I> {
    /// An empty tree.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            vacant: Vec::new(),
            live: 0,
            root: NodeId::NONE,
            focus: Focus::NOWHERE,
        }
    }

    /// The root, or [`NodeId::NONE`] before the first reconcile.
    #[must_use]
    pub const fn root(&self) -> NodeId {
        self.root
    }

    /// Where the focus is.
    #[must_use]
    pub const fn focus(&self) -> Focus {
        self.focus
    }

    /// One node, if that slot is occupied.
    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<&Node<I>> {
        self.nodes.get(id.index()?).and_then(Option::as_ref)
    }

    /// How many nodes there are.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.live
    }

    /// Whether there are none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// A node's children, in declaration order.
    ///
    /// ```
    /// use corvid_ui::{Tree, column, label};
    ///
    /// let mut tree = Tree::<()>::new();
    /// tree.reconcile(column().child(label("one")).child(label("two")));
    /// assert_eq!(tree.children(tree.root()).count(), 2);
    /// ```
    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let first = self.node(id).map_or(NodeId::NONE, |node| node.first_child);
        core::iter::successors(first.is_some().then_some(first), move |current| {
            let next = self.node(*current)?.next_sibling;
            next.is_some().then_some(next)
        })
    }

    /// Every node, parents before children and siblings in declaration order.
    ///
    /// An explicit stack rather than recursion, so a tree ten thousand deep is
    /// a slow frame rather than a blown stack.
    #[must_use]
    pub fn preorder(&self) -> Preorder<'_, I> {
        Preorder {
            tree: self,
            stack: if self.root.is_some() {
                alloc::vec![self.root]
            } else {
                Vec::new()
            },
        }
    }

    /// How many arena slots there are, occupied or not. The bound on a
    /// `NodeId`'s index, which is what an array indexed by one is sized to.
    pub(crate) const fn slots(&self) -> usize {
        self.nodes.len()
    }

    /// The slot a new node goes in, reusing a vacant one when there is one.
    pub(crate) fn allocate(&mut self, node: Node<I>) -> NodeId {
        self.live += 1;
        if let Some(id) = self.vacant.pop()
            && let Some(slot) = id.index().and_then(|index| self.nodes.get_mut(index))
        {
            *slot = Some(node);
            return id;
        }
        self.nodes.push(Some(node));
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a NodeId is a u32 index and NodeId::NONE is u32::MAX, so an arena that reached this cast's limit has already run out of ids; a UI with four billion nodes is not a case this crate is asked to answer"
        )]
        NodeId((self.nodes.len() - 1) as u32)
    }

    /// One node, mutably.
    pub(crate) fn node_mut(&mut self, id: NodeId) -> Option<&mut Node<I>> {
        self.nodes.get_mut(id.index()?).and_then(Option::as_mut)
    }

    /// Drop a node and everything under it, with an explicit stack.
    pub(crate) fn release(&mut self, id: NodeId) {
        let mut stack = alloc::vec![id];
        while let Some(current) = stack.pop() {
            let Some(index) = current.index() else {
                continue;
            };
            let Some(slot) = self.nodes.get_mut(index) else {
                continue;
            };
            let Some(node) = slot.take() else {
                continue;
            };
            self.live -= 1;
            self.vacant.push(current);
            let mut child = node.first_child;
            while child.is_some() {
                stack.push(child);
                child = self.node(child).map_or(NodeId::NONE, |it| it.next_sibling);
            }
        }
    }

    /// Set the root. Used once, by the first reconcile.
    pub(crate) const fn set_root(&mut self, root: NodeId) {
        self.root = root;
    }

    /// Move the focus, without asking whether anything is there.
    pub(crate) const fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
    }
}

impl<I> Default for Tree<I> {
    fn default() -> Self {
        Self::new()
    }
}

/// Hashed as the tree *is*, not as the arena happens to hold it.
///
/// A node removed leaves a vacant slot behind and a later node takes it, so
/// two trees that are the same shape can have different arenas. Hashing the
/// pre-order walk instead means a UI's digest depends on the UI, which is what
/// makes it worth freezing.
impl<I: Hash> Hash for Tree<I> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.live.hash(state);
        self.focus.visible.hash(state);
        for id in self.preorder() {
            if let Some(node) = self.node(id) {
                node.key.hash(state);
                node.kind.hash(state);
                node.style.hash(state);
                (self.focus.node == id).hash(state);
            }
        }
    }
}

/// Every node of a [`Tree`], parents before children.
#[derive(Debug)]
pub struct Preorder<'a, I> {
    tree: &'a Tree<I>,
    stack: Vec<NodeId>,
}

impl<I> Iterator for Preorder<'_, I> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.stack.pop()?;
        let node = self.tree.node(id)?;
        // The sibling first and the child second, so the child comes back off
        // the stack first and the walk is in declaration order.
        if node.next_sibling.is_some() && id != self.tree.root() {
            self.stack.push(node.next_sibling);
        }
        if node.first_child.is_some() {
            self.stack.push(node.first_child);
        }
        Some(id)
    }
}
