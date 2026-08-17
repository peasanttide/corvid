#![doc = include_str!("../README.md")]
#![no_std]

// A stack owns its packs and a listing is a fresh vector of paths, so `alloc`
// is the one thing past `core` this crate needs. Nothing here opens a
// directory: where a pack's files come from is a `corvid_files::Source`, and
// the implementation that reads a real filesystem is a game's own, one layer
// up, where `std` is already paid for.
extern crate alloc;

mod manifest;
mod mount;
mod pack;
mod stack;

pub use manifest::{Manifest, PackId};
pub use mount::Unmountable;
pub use pack::{Pack, PackStamp};
pub use stack::Stack;
