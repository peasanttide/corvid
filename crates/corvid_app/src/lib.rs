#![doc = include_str!("../README.md")]

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
mod game;
mod headless;
// `#[macro_export]` puts what is in here at this crate's root and not under
// this path, so the module is private and the macros are still `corvid_app::game!`.
mod macros;
#[cfg(feature = "net")]
mod net;
mod record;
mod retention;
mod runtime;
mod saves;
#[cfg(feature = "render")]
mod screen;
mod seating;
mod settings;

#[cfg(feature = "window")]
mod windowed;

pub use app::{App, Error, Outcome, Progress};
pub use cli::{Argument, Arguments, Load, main, watch};
pub use commands::{Answer, Command, Request, Requests};
#[cfg(feature = "window")]
pub use controls::Misbound;
pub use game::{AuralizerConfig, BotConfig, ControllerConfig, Game, RenderConfig};
#[cfg(feature = "net")]
pub use net::{Departures, TickTraffic, Traffic, peer_of, seat_of};
pub use retention::Retention;
pub use saves::NotASave;
pub use settings::Settings;

/// What a run answers with: nothing, or why it could not play.
///
/// The default parameter is what a harness driving a run by hand writes -- `fn
/// main() -> corvid_app::Result` around an [`App::launch`] -- and the parameter
/// is there for the calls that hand something back, so that a harness naming
/// this type once does not also have to name [`Error`]. A game whose `main` is
/// [`main`] names neither: that one prints its own reasons and stops the
/// process, so there is nothing left for a return type to carry.
pub type Result<T = ()> = core::result::Result<T, Error>;
