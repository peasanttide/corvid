//! The saturating length maths the three passes share.
//!
//! The seam is that nothing here knows a tree exists. Every function takes the
//! node and the axis only so that a [`TooLarge`] can name where the overflow
//! was, and answers a length or a failure.

use alloc::vec::Vec;

use super::TooLarge;
use crate::{
    arena::NodeId,
    length::{Length, Scale, edge, split},
    style::{Axis, Justify},
    text::{Line, Metrics},
};
use corvid_fixed::I16F16;

/// The main and cross axis of a container, in that order.
pub(super) const fn axes(axis: Axis) -> (Axis, Axis) {
    (axis, axis.across())
}

/// How much space goes before the child at `index`.
pub(super) const fn justify_lead(
    justify: Justify,
    leftover: I16F16,
    index: usize,
    count: usize,
) -> I16F16 {
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
pub(super) fn resolve(
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
pub(super) fn length(
    value: Length,
    scale: Scale,
    node: NodeId,
    axis: Axis,
) -> Result<I16F16, TooLarge> {
    edge(value, scale).ok_or(TooLarge { node, axis })
}

/// `left + right`, or the node that could not hold it.
pub(super) fn add(
    left: I16F16,
    right: I16F16,
    node: NodeId,
    axis: Axis,
) -> Result<I16F16, TooLarge> {
    left.checked_add(right).ok_or(TooLarge { node, axis })
}

/// The same, where the left operand may already have failed to resolve.
pub(super) fn sum(
    left: Option<I16F16>,
    right: I16F16,
    node: NodeId,
    axis: Axis,
) -> Result<I16F16, TooLarge> {
    add(left.ok_or(TooLarge { node, axis })?, right, node, axis)
}

/// `value * count`, or the node that could not hold it.
pub(super) fn times(
    value: I16F16,
    count: usize,
    node: NodeId,
    axis: Axis,
) -> Result<I16F16, TooLarge> {
    value
        .checked_mul(count_as(count))
        .ok_or(TooLarge { node, axis })
}

/// A count as the whole number `I16F16` holds, saturating past 32 767.
pub(super) fn count_as(count: usize) -> I16F16 {
    I16F16::from_bits(
        i32::try_from(count)
            .unwrap_or(i32::MAX)
            .saturating_mul(1 << 16),
    )
}

/// A style's width or height when it is a fixed number of pixels, and nothing
/// when it is a share or as large as its content.
pub(super) const fn fixed(value: Length, scale: Scale) -> Option<I16F16> {
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
pub(super) fn wrapped(
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
