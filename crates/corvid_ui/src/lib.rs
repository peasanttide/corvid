#![doc = include_str!("../README.md")]
#![no_std]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the modules here are private, so pub(crate) and pub are equivalent — pub(crate) is the one that says what is meant, and it is what rustc's unreachable_pub asks for"
)]

// An element tree is built every frame and a solved layout is a list of
// rectangles, so this crate needs an allocator. It needs nothing else: there is
// no `std` here under any feature, and no device under any of them either.
extern crate alloc;

mod arena;
mod build;
mod focus;
mod layout;
mod length;
mod paint;
pub mod style;
mod text;
mod widget;

pub use arena::{Key, Node, NodeId, Preorder, Rebuilt, Tree};
pub use build::Element;
pub use focus::{Compass, Focus, Raised, Signal};
pub use layout::{TooLarge, solve};
pub use length::{Edges, Length, Scale, Size};
pub use paint::{Painted, PaintedGlyph, PaintedNode, PaintedRect, Position, Rect, Visits};
pub use style::{Align, Axis, Justify, Style, TextStyle};
pub use text::{GlyphId, Line, Metrics, Monospace, TooLong};
pub use widget::{Kind, Text, button, column, label, paragraph, row, slider, spacer, toggle};
