//! The doubly linked cycles a triangulation is carved out of.
//!
//! A node is an index into one flat array, and every index this module hands
//! out came from that array, so the parallel fields are always in step. Two
//! nodes may name the same point: bridging a hole and splitting a ring both
//! work by duplicating a node, which is what keeps the *triangles* indexed by
//! the polygon's own points even though the boundary being walked has grown
//! vertices the polygon never had.

use alloc::vec::Vec;

use crate::GroundPoint;

/// One vertex of the boundary being walked.
#[derive(Clone, Copy, Debug)]
struct Node {
    /// Which of the polygon's points this node stands at.
    point: u32,
    prev: u32,
    next: u32,
    live: bool,
}

/// Every node, and the points they stand at.
#[derive(Clone, Debug)]
pub(crate) struct Nodes {
    points: Vec<GroundPoint>,
    nodes: Vec<Node>,
}

impl Nodes {
    /// An empty set of cycles over a polygon's points.
    pub(crate) const fn new(points: Vec<GroundPoint>) -> Self {
        Self {
            points,
            nodes: Vec::new(),
        }
    }

    /// The polygon's points, which is what a triangle's indices name.
    pub(crate) fn points(self) -> Vec<GroundPoint> {
        self.points
    }

    /// Links the points `from .. to` into a cycle, answering its first node,
    /// or `None` when there are fewer than three of them.
    pub(crate) fn link(&mut self, from: usize, to: usize) -> Option<u32> {
        if to.saturating_sub(from) < 3 {
            return None;
        }
        let start = self.nodes.len() as u32;
        let last = (to - from - 1) as u32;
        for offset in 0..=last {
            self.nodes.push(Node {
                point: from as u32 + offset,
                prev: start + if offset == 0 { last } else { offset - 1 },
                next: start + if offset == last { 0 } else { offset + 1 },
                live: true,
            });
        }
        Some(start)
    }

    /// The point a node stands at.
    pub(crate) fn at(&self, node: u32) -> GroundPoint {
        self.points
            .get(self.point(node) as usize)
            .copied()
            .unwrap_or(GroundPoint::ORIGIN)
    }

    /// Which of the polygon's points a node stands at.
    pub(crate) fn point(&self, node: u32) -> u32 {
        self.field(node, |n| n.point)
    }

    /// The node before this one.
    pub(crate) fn prev(&self, node: u32) -> u32 {
        self.field(node, |n| n.prev)
    }

    /// The node after this one.
    pub(crate) fn next(&self, node: u32) -> u32 {
        self.field(node, |n| n.next)
    }

    /// One field of a node, or the node's own index for one that does not
    /// exist.
    ///
    /// A missing node cannot arise -- every index here was handed out by
    /// [`link`](Self::link), [`duplicate`](Self::duplicate) or a link field --
    /// and answering the index itself makes any walk that somehow reached one
    /// terminate immediately rather than run away.
    fn field(&self, node: u32, pick: impl Fn(&Node) -> u32) -> u32 {
        self.nodes.get(node as usize).map_or(node, pick)
    }

    fn set(&mut self, node: u32, mutate: impl FnOnce(&mut Node)) {
        if let Some(node) = self.nodes.get_mut(node as usize) {
            mutate(node);
        }
    }

    /// Takes a node out of its cycle.
    pub(crate) fn unlink(&mut self, node: u32) {
        let (before, after) = (self.prev(node), self.next(node));
        self.set(before, |n| n.next = after);
        self.set(after, |n| n.prev = before);
        self.set(node, |n| n.live = false);
    }

    /// A second node standing at the same point, linked to nothing yet.
    fn duplicate(&mut self, node: u32) -> u32 {
        let index = self.nodes.len() as u32;
        let point = self.point(node);
        self.nodes.push(Node {
            point,
            prev: index,
            next: index,
            live: true,
        });
        index
    }

    fn join(&mut self, before: u32, after: u32) {
        self.set(before, |n| n.next = after);
        self.set(after, |n| n.prev = before);
    }

    /// Splices a hole's cycle into an outer cycle along the diagonal from
    /// `outer` to `hole`.
    ///
    /// The diagonal is walked twice, out along `outer -> hole` and back along
    /// the duplicates, which is what turns two cycles into one without moving
    /// a single point. The two passes are the same segment in opposite
    /// directions, so they enclose nothing and every area and every
    /// containment test downstream is unchanged.
    pub(crate) fn bridge(&mut self, outer: u32, hole: u32) {
        let outer_again = self.duplicate(outer);
        let hole_again = self.duplicate(hole);
        let after_outer = self.next(outer);
        let before_hole = self.prev(hole);

        self.join(outer, hole);
        self.join(before_hole, hole_again);
        self.join(hole_again, outer_again);
        self.join(outer_again, after_outer);
    }

    /// Cuts a cycle in two along the diagonal `a` to `b`, answering the first
    /// node of the half that `a` is not in.
    pub(crate) fn split(&mut self, a: u32, b: u32) -> u32 {
        let a_again = self.duplicate(a);
        let b_again = self.duplicate(b);
        let after_a = self.next(a);
        let before_b = self.prev(b);

        self.join(a, b);
        self.join(a_again, after_a);
        self.join(b_again, a_again);
        self.join(before_b, b_again);
        b_again
    }

    /// The nodes of one cycle, starting at `start`.
    ///
    /// Bounded by the node count, so a cycle that somehow lost its tail ends
    /// the walk rather than the process.
    pub(crate) fn cycle(&self, start: u32) -> Vec<u32> {
        let mut walk = Vec::new();
        let mut node = start;
        for _ in 0..self.nodes.len() {
            walk.push(node);
            node = self.next(node);
            if node == start {
                break;
            }
        }
        walk
    }

    /// Every node still in a cycle, in index order.
    ///
    /// Index order rather than cycle order, because this is the set the
    /// bridging predicates scan and a scan whose order depended on which hole
    /// had been merged already would be a different answer on a different
    /// machine.
    pub(crate) fn live(&self) -> Vec<u32> {
        (0..self.nodes.len() as u32)
            .filter(|&index| self.nodes.get(index as usize).is_some_and(|n| n.live))
            .collect()
    }
}
