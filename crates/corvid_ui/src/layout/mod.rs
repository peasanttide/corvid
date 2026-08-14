//! The flex solver: two passes over the tree, and no more.
//!
//! Pass one walks bottom-up measuring intrinsic content size -- a label asks
//! [`Metrics`], a container sums its children along its axis and takes the
//! maximum across it. Pass two walks top-down distributing free space to
//! `Fraction` children and assigning positions. Nothing iterates to a fixed
//! point, so a layout is O(nodes) and its cost is knowable from the node
//! count alone.
//!
//! The two passes are the seam this module is split along: `measure.rs` is the
//! first, `place.rs` is the second, and `paint.rs` is what the second emits as
//! it goes. `arithmetic.rs` holds the saturating length maths all three share,
//! which is the one part with no opinion about trees at all. Every one of them
//! is an `impl` block on the [`Solver`] declared here, so the state the passes
//! share is written down once.

use alloc::vec::Vec;
use core::hash::Hash;

use crate::{
    arena::{NodeId, Tree},
    length::Scale,
    paint::{Painted, Rect, Visits},
    style::Axis,
    text::Metrics,
};
use corvid_fixed::{Factor16, I16F16};

mod arithmetic;
mod measure;
mod paint;
mod place;
mod share;

/// A resolved length ran past what `I16F16` can hold.
///
/// The saturation `I16F16` would do instead is silent, and a layout that
/// silently clamped is a menu that is subtly wrong on one machine. The node
/// named is the one whose arithmetic ran past the type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[error("a {axis:?} length of node {} ran past what I16F16 holds", node.index().unwrap_or(0))]
pub struct TooLarge {
    /// Whose arithmetic it was.
    pub node: NodeId,
    /// Which axis it was along.
    pub axis: Axis,
}

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
}
