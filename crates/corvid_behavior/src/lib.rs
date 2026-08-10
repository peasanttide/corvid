#![doc = include_str!("../README.md")]
#![no_std]

// A level arrives behind an `Arc` and a bounded name owns its bytes, so this
// crate needs an allocator. It needs nothing else: there is no `std` here under
// any feature.
//
// A tick no longer returns a `Vec<Command>` -- the sink is a `&mut impl` and a
// tick that asks for nothing allocates nothing -- but `Level::load` hands back
// bytes it read, so the allocator stays.
extern crate alloc;

mod command;
mod extract;
mod level;
mod loading;
mod player;
mod state;

pub use command::{
    AchievementId, Command, Discard, ExitCode, LobbyId, PresenceText, RumbleId, SaveSlot, Scope,
    StatId, Url,
};
pub use extract::{Extract, Extracting};
pub use level::Level;
pub use loading::Loading;
pub use player::{PlayerId, PlayerState, Presence, ProfileId};
pub use state::{Data, State};
