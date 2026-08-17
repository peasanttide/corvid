#![doc = include_str!("../README.md")]
#![no_std]

// A run of glyphs, a page of coverage and a list of line breaks are all
// growable, so `alloc` is what this crate needs past `core` -- and the whole of
// what it needs. A face is parsed in place out of borrowed bytes and allocates
// nothing at all.
extern crate alloc;

mod atlas;
mod error;
mod font;
mod kern;
mod raster;
mod scan;
mod shape;
mod wrap;

pub use atlas::{Atlas, Slot};
pub use error::{AtlasFull, FontError};
pub use font::Font;
pub use raster::Coverage;
pub use shape::{NOTDEF, PositionedGlyph, Run, Shaping, shape};
pub use wrap::{Break, Paragraph, Row, wrap};
