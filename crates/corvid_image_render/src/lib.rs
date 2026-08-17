#![doc = include_str!("../README.md")]

// No `no_std`. A texture is created on a device, and this is the layer whose job
// that is; `corvid_image` is the half that has no device in it and builds for a
// target with no operating system.
extern crate alloc;

mod apply;
mod atlas;
mod bindings;
mod budget;
mod cache;
mod error;
mod format;
mod mips;
mod params;
mod shader;
mod table;

pub use atlas::Atlas;
pub use budget::{CACHE_SHARE, FLOOR, resident_tiles, vram_budget, vram_estimate};
pub use cache::TileCache;
pub use error::CacheError;
pub use format::{device_bytes_per_texel, texture_format};
pub use shader::{GROUP, MAX_SOURCES, WGSL, wgsl_at};
