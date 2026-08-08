#![doc = include_str!("../README.md")]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the modules here are private, so pub(crate) and pub are equivalent — pub(crate) is the one that says what is meant, and it is what rustc's unreachable_pub asks for"
)]

// No `#![no_std]`, and no feature that would make one possible. This crate
// opens directories, writes files and reads a clock, and it is the layer whose
// job that is: everything below it is `no_std` precisely because everything
// that touches an operating system was pushed up here.

mod app;
mod backend;
mod capture;
mod cli;
mod commands;
// The player's own binding table, which only a windowed run has one of.
#[cfg(feature = "window")]
mod controls;
mod headless;
#[cfg(feature = "net")]
mod net;
mod retention;
mod runtime;
mod saves;
#[cfg(feature = "render")]
mod screen;
mod settings;

#[cfg(feature = "window")]
mod windowed;

pub use app::{App, Error, Outcome, Progress};
pub use cli::{Argument, Arguments, main, watch};
pub use commands::{Answer, Command, Request, Requests};
#[cfg(feature = "window")]
pub use controls::Misbound;
#[cfg(feature = "net")]
pub use net::{Departures, TickTraffic, Traffic, seat_of};
pub use retention::Retention;
pub use saves::NotASave;
pub use settings::Settings;

/// What a run answers with: nothing, or why it could not play.
///
/// The default parameter is what a game's `main` writes — `fn main() ->
/// corvid_app::Result` — and the parameter is there for the calls that hand
/// something back, so that a harness naming this type once does not also have
/// to name [`Error`].
pub type Result<T = ()> = core::result::Result<T, Error>;
