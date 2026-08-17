#![doc = include_str!("../README.md")]
#![no_std]

// A picture is a buffer and a plan is four ordered collections, so `alloc` is
// what this crate needs past `core` -- and, but for one codec, the whole of it.
extern crate alloc;

// The `png` crate reads through `std::io::Read` and offers no way to ask it
// not to, which is why the `png` feature implies `std`. Nothing else here does.
#[cfg(feature = "std")]
extern crate std;

mod config;
mod decode;
mod error;
mod format;
mod image;
mod plan;
mod planner;
mod residency;
mod source;
mod table;
mod tile;
mod view;

pub use config::{
    MAX_IMAGE_SIZE, MAX_NUM_MAPS, MAX_NUM_TILES, MIN_SPEC_VRAM, TILE_SIZE, TileConfig, VramBudget,
};
pub use decode::decode;
#[cfg(feature = "jpeg")]
pub use decode::decode_jpeg;
#[cfg(feature = "png")]
pub use decode::decode_png;
pub use error::{ConfigError, DecodeError, ImageError, TileError};
pub use format::{Channels, Codec, ColorSpace, PixelFormat};
pub use image::{Extent, Image, extent};
pub use plan::{Eviction, TilePlan, Upload};
pub use planner::TilePlanner;
pub use residency::Residency;
pub use source::{Source, Sources};
pub use table::{TileSample, TileTable};
pub use tile::{SourceId, TileEntry, TileKey, TileSlot};
pub use view::{Priority, SourceView, Tier, UvRect};
