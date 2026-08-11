//! Numbering a declaration: how the runs of identifiers are handed out.
//!
//! Split from [the declaration](super) because a file stays under 400 lines,
//! and this is the seam that was already there: everything in the parent
//! describes a table that already exists, and this is what builds one. It is
//! also the only part of the crate that runs at compile time and nowhere else.

use super::{IdRange, SetDescriptor, SetNames};
use crate::id::SetId;

/// Turns declaration order into identifiers.
///
/// Sets are numbered from zero in the order they arrive, and each kind of
/// action is numbered from zero in its own space, so the ranges of a kind
/// partition that space in declaration order with no gaps. That is the whole of
/// the numbering rule, and it is a wire format: a binding file saved by
/// yesterday's build names these numbers, so moving a declaration re-points
/// every binding at or after it.
///
/// ```
/// use corvid_input::{IdRange, SetNames, layout};
///
/// const TABLE: [corvid_input::SetDescriptor; 2] = layout(&[
///     SetNames {
///         name: "Menu",
///         digital: &["UP", "DOWN", "ACTIVATE", "BACK"],
///         analog: &[],
///         pose: &[],
///     },
///     SetNames {
///         name: "Build",
///         digital: &["PLACE", "CANCEL"],
///         analog: &["LOOK", "MOVE"],
///         pose: &["POINTER"],
///     },
/// ]);
///
/// // The second set's digital actions continue where the first's stopped, and
/// // its analog actions start over, because the two kinds are numbered apart.
/// assert_eq!(TABLE[1].digital(), IdRange::new(4, 2));
/// assert_eq!(TABLE[1].analog(), IdRange::new(0, 2));
///
/// // And the names came along, which is what a binding file writes down.
/// assert_eq!(corvid_input::digital_named(&TABLE, "PLACE"),
///            Some(corvid_input::DigitalId(4)));
/// ```
///
/// # Panics
///
/// When a declaration has more identifiers of one kind than a `u16` can
/// number. From a `const` item -- which is the only place
/// [`action_sets!`](crate::action_sets) calls this -- that panic is a compile
/// error, and it is one in every profile:
///
/// ```rust,compile_fail
/// use corvid_input::{SetDescriptor, SetNames, layout};
///
/// const TOO_MANY: [SetDescriptor; 2] = layout(&[
///     SetNames { name: "First", digital: &["a"; 40_000], analog: &[], pose: &[] },
///     SetNames { name: "Second", digital: &["b"; 40_000], analog: &[], pose: &[] },
/// ]);
/// assert_eq!(TOO_MANY.len(), 2);
/// ```
///
/// It is a panic rather than a wrapping `+` because the two are not the same
/// thing here. Const evaluation rejects a `+` that overflows only when the
/// profile has overflow checks on, so plain arithmetic would have made this a
/// compile error in a `dev` build and a table of overlapping identifiers in a
/// `release` one -- the same declaration numbered two different ways by two
/// builds of the same game, which is the one outcome a wire format cannot have.
#[must_use]
pub const fn layout<const N: usize>(counts: &[SetNames; N]) -> [SetDescriptor; N] {
    let mut table = [SetDescriptor::EMPTY; N];
    let mut index = 0;
    let mut id: u16 = 0;
    let mut digital: u16 = 0;
    let mut analog: u16 = 0;
    let mut pose: u16 = 0;

    while index < N {
        let set = &counts[index];
        // A kind's count is how many names it declared. `usize` to `u16` is
        // the one narrowing here, and it refuses rather than wrapping for the
        // reason `advance` does.
        let digital_count = count(set.digital.len());
        let analog_count = count(set.analog.len());
        let pose_count = count(set.pose.len());
        table[index] = SetDescriptor::new(
            SetId(id),
            *set,
            IdRange::new(digital, digital_count),
            IdRange::new(analog, analog_count),
            IdRange::new(pose, pose_count),
        );
        digital = advance(digital, digital_count);
        analog = advance(analog, analog_count);
        pose = advance(pose, pose_count);
        index += 1;
        // The last set has no successor to number, and asking for one would be
        // the only step in this loop that could refuse a declaration that was
        // otherwise fine.
        if index < N {
            id = advance(id, 1);
        }
    }

    table
}

/// How many identifiers a slice of names is, as a `u16`.
///
/// # Panics
///
/// When a set declares more than 65 535 actions of one kind. A compile error
/// from a `const` item, for the reason [`advance`] gives.
#[allow(
    clippy::panic,
    reason = "const evaluation turns this into a compile error, which is the whole point: see `advance`"
)]
#[allow(
    clippy::cast_possible_truncation,
    reason = "the range pattern is what makes the cast exact"
)]
const fn count(names: usize) -> u16 {
    match names {
        0..=0xFFFF => names as u16,
        _ => panic!("an action set declares more actions of one kind than a u16 can number"),
    }
}

/// Moves a running total on, refusing to wrap.
///
/// # Panics
///
/// When the total does not fit in a `u16`. See [`layout`], which is the only
/// caller and whose documentation explains why this is a panic.
#[allow(
    clippy::panic,
    reason = "const evaluation turns this into a compile error, which is the whole point: it is the only way to refuse an overflowing declaration in a profile that has turned overflow checks off, and the workspace ships one of those"
)]
const fn advance(total: u16, by: u16) -> u16 {
    match total.checked_add(by) {
        Some(next) => next,
        None => panic!(
            "an action set declaration has more identifiers of one kind than a u16 can number"
        ),
    }
}
