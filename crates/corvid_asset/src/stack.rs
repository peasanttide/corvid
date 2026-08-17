//! The mounted set: one filesystem out of several, and one number standing for
//! which several.

use alloc::{string::String, vec::Vec};
use core::hash::Hash;

use corvid_files::{Missing, Source};
use corvid_hash::{Digest, Hasher};

use crate::{Pack, PackStamp, Unmountable, mount};

/// Packs in mount order, read as one filesystem.
///
/// Index zero is the bottom and the last is the top. A read walks down from the
/// top and stops at the first pack holding the path, so a pack mounted later
/// overrides an earlier one by using the same path and by nothing else: there
/// is no override table, no priority number and no way for a pack to reach a
/// file except by naming where it lives.
///
/// A `Stack` is itself a [`Source`], which is what makes it the thing a level is
/// handed. It inherits `Source::write`'s refusing default, so the stack a level
/// loads through cannot be written to at all -- the packs under it are somebody
/// else's content and a load is not the moment to edit them.
#[derive(Debug, Default)]
pub struct Stack {
    packs: Vec<Pack>,
}

impl Stack {
    /// Nothing mounted.
    #[must_use]
    pub const fn new() -> Self {
        Self { packs: Vec::new() }
    }

    /// Resolves an order for `packs` and mounts them in it.
    ///
    /// The order offered is the order kept wherever the requirements allow it,
    /// and a pack is pulled below whatever requires it where they do not. See
    /// [`Unmountable`] for what a set that has no order at all does instead of
    /// looping.
    ///
    /// # Errors
    ///
    /// [`Unmountable::Twice`] for two packs with one identifier,
    /// [`Unmountable::Absent`] for a requirement nothing in the set answers to,
    /// and [`Unmountable::Cycle`] for requirements that lead back to where they
    /// started.
    pub fn mount(packs: Vec<Pack>) -> Result<Self, Unmountable> {
        let order = mount::order(&packs)?;
        let mut offered: Vec<Option<Pack>> = packs.into_iter().map(Some).collect();
        let mut mounted = Vec::with_capacity(offered.len());
        for at in order {
            // Every index the resolver answers is in range and appears once, so
            // this always takes a pack. Written as a conditional rather than an
            // index so that a resolver that ever broke that promise mounts a
            // shorter stack instead of aborting the process.
            if let Some(pack) = offered.get_mut(at).and_then(Option::take) {
                mounted.push(pack);
            }
        }
        Ok(Self { packs: mounted })
    }

    /// The packs, bottom first.
    #[must_use]
    pub fn packs(&self) -> &[Pack] {
        &self.packs
    }

    /// How many are mounted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.packs.len()
    }

    /// Whether none are.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.packs.is_empty()
    }

    /// Each pack's identifier and version, in mount order.
    ///
    /// What a game puts in its rules, where it travels to every peer inside the
    /// opening and is compared before a tick is simulated.
    #[must_use]
    pub fn stamps(&self) -> Vec<PackStamp> {
        self.packs.iter().map(Pack::stamp).collect()
    }

    /// One number for the identity of the whole mounted set.
    ///
    /// This is the value two peers compare at seating. It absorbs each pack's
    /// identifier and version in mount order, so a peer with an extra mod, a
    /// peer missing one, a peer on an older version of one, and a peer who
    /// mounted the same three in a different order all disagree with everybody
    /// else immediately and by name, rather than agreeing for forty seconds and
    /// then diverging somewhere no log explains.
    ///
    /// The count goes in first, which is what keeps an empty stack from
    /// colliding with one whose single stamp happens to absorb to the same
    /// state. Order is part of it because a stack is a list: the same packs
    /// mounted the other way round override each other the other way round and
    /// are a different game.
    ///
    /// Nothing about the files is in here. A pack that shipped edited bytes
    /// under an unchanged version digests the same, and catching that is
    /// [`Pack::content`], which reads every file and belongs to a build rather
    /// than to a lobby.
    #[must_use]
    pub fn digest(&self) -> Digest {
        let mut hasher = Hasher::new();
        self.packs.len().hash(&mut hasher);
        for pack in &self.packs {
            pack.stamp().hash(&mut hasher);
        }
        hasher.digest()
    }

    /// Which pack a path reads out of, or none if no pack holds it.
    ///
    /// The diagnostic half of [`read`](Source::read): a level that loaded the
    /// wrong material is a question about which pack won, and answering it by
    /// hand means re-implementing the walk.
    #[must_use]
    pub fn provider(&self, path: &str) -> Option<&Pack> {
        self.packs
            .iter()
            .rev()
            .find(|pack| pack.source().exists(path))
    }
}

impl Source for Stack {
    /// The topmost pack's copy of the file.
    ///
    /// # Errors
    ///
    /// [`Missing`] when no pack in the stack holds the path. A pack that could
    /// not be read is not distinguished from one that has nothing there, which
    /// is `Source::read`'s own arrangement: the finding is that the bytes did
    /// not arrive, and here there is a pack below that may still have them.
    fn read(&self, path: &str) -> Result<Vec<u8>, Missing> {
        self.packs
            .iter()
            .rev()
            .find_map(|pack| pack.source().read(path).ok())
            .ok_or_else(|| Missing::new(path))
    }

    /// Every path any pack holds, once each, sorted.
    ///
    /// **Sorted, and that is load-bearing.** Each pack answers its own listing
    /// in order, but the union of several ordered lists has no order of its own
    /// and de-duplicating one that arrived in mount order would leave the
    /// result depending on which pack happened to define a path first. Sorting
    /// makes the listing a function of the set of paths alone, so two peers
    /// with the same content walk it identically -- and paths are compared as
    /// bytes, which is the same comparison on every platform. A game that built
    /// its level out of whatever `list` returned would otherwise hash a
    /// different level on a machine that mounted its mods in a different order.
    ///
    /// De-duplicated, because a path two packs define is one file: the override
    /// happened, and a listing that named it twice would have a caller load it
    /// twice and keep the loser.
    ///
    /// # Errors
    ///
    /// [`Missing`] if any pack cannot be listed. One pack that cannot say what
    /// is in it makes the union unknowable, and answering with the rest would be
    /// answering a different question quietly.
    fn list(&self) -> Result<Vec<String>, Missing> {
        let mut paths = Vec::new();
        for pack in &self.packs {
            paths.extend(pack.source().list()?);
        }
        paths.sort_unstable();
        paths.dedup();
        Ok(paths)
    }

    /// Whether any pack holds the path.
    ///
    /// Overridden rather than left to the default, which would read the file and
    /// throw the bytes away: every pack under this one can answer the question
    /// from a key, and copying a level's largest asset out of an archive to find
    /// out whether it is there is not a thing to do once per path.
    fn exists(&self, path: &str) -> bool {
        self.packs.iter().any(|pack| pack.source().exists(path))
    }
}
