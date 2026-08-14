//! How an action log is compared, hashed and written down.
//!
//! All four are hand-written, and all four for the same reason: the generation
//! counter is not part of what a log *is*. It counts corrections taken on this
//! machine, so two peers holding the same session disagree about it -- and a
//! derived `PartialEq` or `Hash` would make that disagreement a desync, while a
//! derived encoding would put a local number into a capture other machines read.
//!
//! Kept beside the type rather than in it because these are four answers to one
//! question, and reading them together is what makes the omission obvious.

use alloc::vec::Vec;
use core::hash::{Hash, Hasher};

use corvid_time::Tick;
use serde::Deserialize;

use super::ActionLog;

/// The actions and where they sit, and never the generation.
///
/// Two logs are equal when they record the same session. The generation is one
/// machine's bookkeeping about its own snapshot ring, so a log written down at
/// generation forty and read back at zero is the same log -- which is what
/// `tests/roundtrip.rs` asserts by comparing a saved session against the loaded
/// one.
impl<A: PartialEq> PartialEq for ActionLog<A> {
    fn eq(&self, other: &Self) -> bool {
        self.first == other.first
            && self.players == other.players
            && self.actions == other.actions
            && self.confirmed == other.confirmed
    }
}

/// The same four fields [`PartialEq`] compares, in the same order.
///
/// Hand-written rather than derived, for the reason directly above: a derived
/// [`Hash`] would absorb the generation, and two logs that record the same
/// session and compare equal would then hash apart -- which is the one thing an
/// implementation of this trait is not allowed to do.
impl<A: Hash> Hash for ActionLog<A> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.first.hash(state);
        self.players.hash(state);
        self.actions.hash(state);
        self.confirmed.hash(state);
    }
}

/// The four fields a log is written down as, which is every field but the
/// generation.
#[derive(Deserialize)]
struct Wire<A> {
    /// The tick the first row belongs to.
    first: Tick,
    /// How many seats wide a row is.
    players: u16,
    /// The entries, row-major.
    actions: Vec<A>,
    /// The confirmation bitmap.
    confirmed: Vec<u8>,
}

/// Reads the four recorded fields and gives the log a generation of its own.
///
/// A decoded log has taken no corrections *on this machine*, whatever it took on
/// the one that recorded it, and there is no snapshot ring here that could be
/// stale against those. So every row starts at zero -- one entry per row, because
/// a generation that was left empty would silently stop counting the corrections
/// a rollback then makes.
impl<'de, A: Deserialize<'de>> Deserialize<'de> for ActionLog<A> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Wire::<A>::deserialize(deserializer)?;
        let mut log = Self {
            first: wire.first,
            players: wire.players,
            actions: wire.actions,
            confirmed: wire.confirmed,
            corrections: Vec::new(),
        };
        // Bounded by the entries that have already been decoded, so this cannot
        // be asked for more than the actions themselves already cost.
        let rows = usize::try_from(log.ticks()).unwrap_or(usize::MAX);
        log.corrections.resize(rows, 0);
        Ok(log)
    }
}
