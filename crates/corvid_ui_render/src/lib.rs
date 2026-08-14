#![doc = include_str!("../README.md")]

// No `no_std`. A pipeline is created on a device, and this is the layer whose
// job that is; `corvid_ui` is the half that has no device in it and builds for
// a target with no operating system.
extern crate alloc;

mod atlas;
mod batch;
mod instance;
mod painter;
mod scissor;

pub use atlas::{Atlas, Grid};
pub use batch::{Batch, batches};
pub use instance::{GlyphInstance, RectInstance};
pub use painter::Painter;
pub use scissor::scissor;

/// The shader both pipelines are built from.
const SHADER: &str = include_str!("ui.wgsl");

/// How many vertices a quad drawn as a triangle strip is.
const QUAD: u32 = 4;
