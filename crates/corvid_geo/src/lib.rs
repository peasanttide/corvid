#![doc = include_str!("../README.md")]
#![no_std]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    reason = "moving between the binary scales a geodetic conversion needs is this crate's subject matter; every cast is preceded by a range check, a saturating conversion, or a bound stated in the comment above it"
)]

// Rings, holes and triangles are all growable lists, so `alloc` is what this
// crate needs past `core` -- and the whole of what it needs, until the
// `project` feature asks for an operating system to compute a logarithm on.
extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod anchor;
mod arith;
mod ellipsoid;
mod geodetic;
mod ground;
mod polygon;
#[cfg(feature = "project")]
mod project;

pub use anchor::Anchor;
pub use ellipsoid::Ellipsoid;
pub use geodetic::Geodetic;
pub use ground::{GroundPoint, Winding, ground};
pub use polygon::{Polygon, Ring, Triangulate, Triangulation};

#[cfg(feature = "project")]
pub use project::{ConformalConic, Projected, Wgs84};
