//! The fixtures the two frozen tables are recorded over.
//!
//! `tests/golden.rs` freezes what these digest to and `tests/wire.rs` freezes
//! what they serialize to, and the two encodings share nothing -- one is
//! hand-written and one is derived -- so the fixtures have to be the same values
//! or the two tables are talking about different things. They live here for
//! that reason rather than for brevity.
//!
//! # What is not frozen here
//!
//! Neither a command nor a save. `Command` is a trait with one method per
//! effect rather than a closed enum with discriminants to pin, and a save
//! carries no bytes to order. Nothing is lost by their absence -- a command was
//! never part of a session's digest, which is taken over the state alone -- but
//! it is worth saying that the tables below
//! are now about the roster and the identifiers, which do cross a wire.

use corvid_behavior::{PlayerId, PlayerState, Presence, ProfileId};

use corvid_time::Tick;

/// Every [`Presence`], including the one with no payload.
pub(crate) fn every_presence() -> Vec<Presence> {
    vec![
        Presence::Joining {
            profile: ProfileId(77),
        },
        Presence::Active,
        Presence::Dropped { since: Tick(4) },
    ]
}

/// The same seat twice, once dropped and once active, so that a table pins the
/// presence as part of a player rather than as something carried alongside one.
///
/// Every field holds a different value from every other, which is what makes
/// the order visible: a [`PlayerState`] written backwards would still encode to
/// something, and would still tell two players apart.
pub(crate) fn every_player(action: u32) -> Vec<PlayerState<u32>> {
    vec![
        PlayerState {
            id: PlayerId(2),
            presence: Presence::Dropped { since: Tick(5) },
            action,
        },
        PlayerState {
            id: PlayerId(2),
            presence: Presence::Active,
            action,
        },
    ]
}
