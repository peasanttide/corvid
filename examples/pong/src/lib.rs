#![doc = include_str!("../README.md")]

#[cfg(feature = "render")]
mod art;
pub mod bot;
mod play;
#[cfg(feature = "net")]
pub mod rally;
mod table;

#[cfg(feature = "render")]
pub use art::{Graphics, ball_at, empty};
pub use play::{
    CHIME, Ears, FLASH, Hands, KNOCK, THUD, action, court, opening, origin, rules, schema,
};
pub use table::{
    Ball, Contact, Court, Level, Move, NoSuchLevel, Paddle, Play, Pong, SEATS, Table, index,
};

/// How fast this game ticks.
///
/// Thirty a second, rather than the workspace's fifteen-hertz default. Pong is
/// a game of reacting to a ball, and a paddle that can only change direction
/// twice in a tenth of a second reads as a paddle that is fighting you. It is
/// also the more interesting rate for the netcode: at thirty hertz a domestic
/// link's latency is two or three ticks rather than one, so prediction has
/// something to do.
pub const RATE: corvid::TickSpan = rate(30);

/// A rate from a literal, with no panic in sight.
///
/// The workspace denies `panic`, `unwrap` and `expect` alike, so a non-zero
/// constant cannot be written down with any of them. One hertz is the slowest
/// rate there is, so a constant somebody edited to zero becomes a very slow
/// game rather than a build that stops.
const fn rate(hz: u32) -> corvid::TickSpan {
    match core::num::NonZeroU32::new(hz) {
        Some(hz) => corvid::TickSpan::from_hz(hz),
        None => corvid::TickSpan::from_hz(core::num::NonZeroU32::MIN),
    }
}

// Nothing to draw, on a build without this game's own renderer. The view is the
// same one either way — `Present` sits on `Render`, so a game states what it
// draws with whether or not it has anything to draw.
#[cfg(not(feature = "render"))]
impl corvid::Render for Pong {
    type Graphics = ();
}
