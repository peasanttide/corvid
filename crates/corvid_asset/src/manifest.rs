//! What a pack says about itself before anything reads a file out of it.

use alloc::{string::String, vec::Vec};

corvid_name::bounded_name! {
    /// What a pack answers to, everywhere and for ever.
    ///
    /// Thirty-two bytes, which is a wire-format decision rather than a guess at
    /// a comfortable length: a `PackId` goes into a game's rules and therefore
    /// into the opening two peers compare, so the capacity is part of what they
    /// agree on and widening it later moves every digest. Thirty-two holds the
    /// flat kebab-case identifiers content is addressed by -- `terminus`,
    /// `reveillon-riots` -- with room left, and refuses anything longer rather
    /// than cutting it, because two packs cut to the same identifier would
    /// override each other's files in silence.
    PackId, 32
}

/// A pack's identity, its human name, its version, and what it needs mounted
/// under it.
///
/// There is nothing here about where the files are. A manifest is the record a
/// pack states about itself and a [`Pack`](crate::Pack) is that record bolted
/// to the [`Source`](corvid_files::Source) it is read through, so the same
/// manifest describes a directory, an archive and a map in memory without
/// knowing which it got.
///
/// [`requires`](Self::requires) is a hard dependency: a
/// [`Stack::mount`](crate::Stack::mount) that cannot find one refuses rather
/// than mounting a pack whose overrides land on nothing. It is also what fixes
/// the order, since a pack that requires another is a pack that overrides it
/// and therefore has to sit above it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Manifest {
    /// What this pack is, in the vocabulary every other pack names it by.
    pub id: PackId,
    /// What it is called where a person reads it.
    ///
    /// A plain `String`, because a menu is a client concern and localizing one
    /// is a game's own table rather than a field a digest has an opinion about.
    pub name: String,
    /// Which revision of this pack it is.
    ///
    /// Part of [`Stack::digest`](crate::Stack::digest), so publishing a fix
    /// without turning this over leaves two peers with different bytes and the
    /// same answer when they compare. Turning it over is what makes the
    /// mismatch loud.
    pub version: u32,
    /// The packs that must be mounted below this one.
    #[cfg_attr(feature = "serde", serde(default))]
    pub requires: Vec<PackId>,
}

impl Manifest {
    /// A pack that requires nothing.
    #[must_use]
    pub fn new(id: PackId, name: impl Into<String>, version: u32) -> Self {
        Self {
            id,
            name: name.into(),
            version,
            requires: Vec::new(),
        }
    }

    /// The same, needing one more pack under it.
    ///
    /// Chained rather than taking a list, because the common manifest names one
    /// requirement and the uncommon one can push onto
    /// [`requires`](Self::requires) directly.
    #[must_use]
    pub fn requiring(mut self, id: PackId) -> Self {
        self.requires.push(id);
        self
    }
}
