#![doc = include_str!("../README.md")]

// No `no_std`. This crate talks to a windowing system and holds a window in an
// `Arc` that a renderer on another line of the same thread also holds.

mod host;
// Gamepads, which `winit` does not read. The only module in the workspace that
// names a pad backend.
mod motion;
#[cfg(feature = "gamepad")]
mod pad;
mod run;
mod state;
mod surface;
mod translate;

pub use host::{Attached, Config, Flow, Host};
pub use run::{Error, Opening, run};
pub use state::{Size, SurfaceState};
pub use surface::Surface;

// The windowing library, whole, because a game with a platform-specific need --
// a fullscreen mode, an icon, an Android lifecycle hook -- has to reach it, and
// pinning it here is what keeps one `winit` in the graph.
pub use winit;
