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
pub use bot::Opponent;
pub use play::{
    CHIME, Ears, FLASH, Hands, KNOCK, THUD, action, court, opening, origin, rules, schema,
};
pub use table::{
    Ball, Contact, Court, Level, Move, NoSuchLevel, Paddle, Play, SEATS, Table, index,
};
