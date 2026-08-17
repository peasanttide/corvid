//! Turning a set of packs into an order, and the three ways that fails.

use alloc::{collections::BTreeSet, vec::Vec};
use core::fmt;

use crate::{Pack, PackId};

/// Why a set of packs is not a stack.
///
/// Every one of these is a refusal at mount time rather than a hole discovered
/// later. A pack whose requirement is absent would sit above nothing and
/// override nothing; a duplicated identifier would make "the pack called this"
/// ambiguous in every other pack's `requires`; and a cycle has no order at all,
/// so the resolver reports it rather than walking round it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum Unmountable {
    /// A pack requires one that is not in the set.
    #[error("{by} requires {needs}, which is not in the stack")]
    Absent {
        /// The pack that asked.
        by: PackId,
        /// What it asked for.
        needs: PackId,
    },
    /// Two packs claim the same identifier.
    #[error("{id} is in the stack twice")]
    Twice {
        /// The identifier both of them claim.
        id: PackId,
    },
    /// Requirements that lead back to where they started.
    #[error(fmt = cycle)]
    Cycle {
        /// Everything left when nothing more could be mounted, in the order it
        /// was offered. The cycle is in here; so is anything that requires it.
        packs: Vec<PackId>,
    },
}

/// How [`Unmountable::Cycle`] reads, which is a list rather than a sentence.
///
/// Naming one pack would be naming an arbitrary member of a loop, and naming
/// none would leave a person to work out which of forty mods is the problem, so
/// the message is every pack that could not be placed.
#[allow(
    clippy::ptr_arg,
    reason = "the signature is the derive's rather than this crate's: `#[error(fmt = ...)]` hands each field of the variant over by reference, and a slice does not typecheck against it"
)]
fn cycle(packs: &Vec<PackId>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("these packs require each other and cannot be ordered:")?;
    for id in packs {
        write!(f, " {id}")?;
    }
    Ok(())
}

/// The order `packs` mounts in, as indices into it.
///
/// The caller's order is the intent and is kept wherever the requirements allow
/// it: of everything that could be mounted next, the one offered earliest goes
/// next. That is what makes the answer a statement about what the caller asked
/// for -- swapping two independent packs swaps them here and changes the digest,
/// which is the point, since two peers who mounted the same mods in different
/// orders are not playing the same game. Sorting the ties by identifier instead
/// would have made the load order alphabetical and unable to express that at
/// all.
///
/// What the requirements do is pull a pack below the packs that need it. A
/// caller who mounted a level before the base it requires gets the base first
/// and a working stack, because "requires" already says which way round they go
/// and making the caller say it twice is a second thing to get wrong.
///
/// The scan is quadratic in the number of packs, which is a number of mods
/// rather than a number of files, and it runs once per session.
///
/// # Errors
///
/// [`Unmountable::Twice`] for two packs with one identifier,
/// [`Unmountable::Absent`] for a requirement no pack in the set answers to, and
/// [`Unmountable::Cycle`] when packs remain and none of them can go next.
pub(crate) fn order(packs: &[Pack]) -> Result<Vec<usize>, Unmountable> {
    let mut offered = BTreeSet::new();
    for pack in packs {
        let id = pack.manifest().id;
        if !offered.insert(id) {
            return Err(Unmountable::Twice { id });
        }
    }

    // Separately from the walk below, so that a missing requirement is reported
    // as the missing requirement it is. Left to the walk it would surface as a
    // cycle, which is the same refusal with a message that sends somebody
    // looking for a loop that is not there.
    for pack in packs {
        for needs in &pack.manifest().requires {
            if !offered.contains(needs) {
                return Err(Unmountable::Absent {
                    by: pack.manifest().id,
                    needs: *needs,
                });
            }
        }
    }

    let mut mounted = BTreeSet::new();
    let mut order = Vec::with_capacity(packs.len());
    while order.len() < packs.len() {
        let next = packs.iter().enumerate().find(|(_, pack)| {
            let manifest = pack.manifest();
            !mounted.contains(&manifest.id)
                && manifest
                    .requires
                    .iter()
                    .all(|needs| mounted.contains(needs))
        });
        // Identifiers are unique by the check above, so "already mounted" is a
        // question the set can answer and no index has to be marked.
        let Some((at, pack)) = next else {
            return Err(Unmountable::Cycle {
                packs: packs
                    .iter()
                    .map(|pack| pack.manifest().id)
                    .filter(|id| !mounted.contains(id))
                    .collect(),
            });
        };
        mounted.insert(pack.manifest().id);
        order.push(at);
    }
    Ok(order)
}
