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
mod arguments;
mod backend;
mod capture;
mod commands;
// The player's own binding table, which only a windowed run has one of.
#[cfg(feature = "window")]
mod controls;
mod entry;
mod headless;
#[cfg(feature = "net")]
mod net;
mod retention;
mod runtime;
mod saves;
#[cfg(feature = "render")]
mod screen;

#[cfg(feature = "window")]
mod windowed;

#[cfg(feature = "dev")]
pub mod dev;

pub use app::{App, Error, Outcome, Progress};
pub use arguments::{Argument, Arguments};
pub use commands::{Answer, Command, Request, Requests};
#[cfg(feature = "window")]
pub use controls::Misbound;
pub use entry::main;
#[cfg(feature = "net")]
pub use net::{Departures, Played, Traffic, seat_of};
pub use retention::Retention;
pub use saves::NotASave;

/// What a run answers with: nothing, or why it could not play.
///
/// The default parameter is what a game's `main` writes — `fn main() ->
/// corvid_app::Result` — and the parameter is there for the calls that hand
/// something back, so that a harness naming this type once does not also have
/// to name [`Error`].
pub type Result<T = ()> = core::result::Result<T, Error>;

/// What this build of the runtime computes under, as a name to record beside
/// anything frozen.
///
/// `"dev"` for a build with the [`dev`](self#the-dev-feature) feature and
/// `"plain"` for one without. The two are documented to compute different
/// states for a game that reads its scratch, and neither is wrong — so a
/// capture, a hash trace or a golden directory blessed under one of them is
/// evidence about that one and about nothing else, and a comparison across the
/// pair is a red test that says the arithmetic moved when what moved was the
/// feature.
///
/// It is a `&'static str` rather than an enum because the thing that reads it
/// is a file name or a header in somebody's golden format, and because a
/// version of this crate with a third configuration should be able to add a
/// name without a downstream `match` becoming non-exhaustive.
///
/// ```
/// // A golden directory says which build blessed it, so a run under the other
/// // one refuses to compare rather than reporting a wire-format break.
/// let blessed_under = corvid_app::flavour();
/// assert!(matches!(blessed_under, "dev" | "plain"));
/// ```
#[must_use]
pub const fn flavour() -> &'static str {
    if cfg!(feature = "dev") {
        "dev"
    } else {
        "plain"
    }
}
