#![doc = include_str!("../README.md")]
#![no_std]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the modules here are private, so pub(crate) and pub are equivalent — pub(crate) is the one that says what is meant, and it is what rustc's unreachable_pub asks for"
)]

// A level arrives behind an `Arc` and a bounded name owns its bytes, so this
// crate needs an allocator. It needs nothing else: there is no `std` here under
// any feature.
//
// A tick no longer returns a `Vec<Command>` — the sink is a `&mut impl` and a
// tick that asks for nothing allocates nothing — but `Level::load` hands back
// bytes it read, so the allocator stays.
extern crate alloc;

mod command;
mod extract;
mod faithful;
mod id;
mod level;
mod name;
mod player;
mod state;
mod time;

pub use command::{
    AchievementId, Command, Discard, ExitCode, LobbyId, PresenceText, RumbleId, SaveSlot, Scope,
    StatId, Url,
};
pub use extract::Extract;
pub use faithful::{Unfaithful, round_trip_is_faithful};
pub use level::Level;
pub use name::InvalidName;
pub use player::{Player, PlayerId, Presence, ProfileId};
pub use state::{Data, State};
pub use time::{Loading, Time};
