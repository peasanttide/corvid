#![doc = include_str!("../README.md")]
#![no_std]

// A mesh is a growable list of vertices and a growable list of indices, so
// `alloc` is the one thing past `core` this crate needs — and it is the whole
// of what it needs, which is why it builds for a target with no operating
// system.
extern crate alloc;

mod mesh;
mod shapes;
mod vertex;

pub use mesh::Mesh;
pub use shapes::{cone, cube, cylinder, grid, icosphere, quad};
pub use vertex::Vertex;
