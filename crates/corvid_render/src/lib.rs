#![doc = include_str!("../README.md")]

// No `no_std`. A device is opened, a window is talked to and a frame is read
// back through a channel, and this is the layer whose job that is.

mod icon;
mod render;
mod renderer;

pub use icon::{Icon, NotAnIcon};
pub use render::{Drawing, Opened, Render, Target};
pub use renderer::{
    Error, Extent, Image, Pacing, Renderer, SOFTWARE, Unacquired, adapter_is_software,
};

// The device, whole, because a game records real `wgpu` and needs every name in
// it -- and because pinning it here is what keeps one `wgpu` in the graph.
pub use wgpu;
