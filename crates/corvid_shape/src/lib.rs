#![doc = include_str!("../README.md")]
#![no_std]

// No `extern crate alloc`. Every shape here is a fixed number of points, and a
// cast answers one hit or none. A caller with a mesh owns the list; this crate
// is what it casts each face at.

mod aabb;
mod frustum;
mod plane;
mod ray;
mod sphere;
mod triangle;

pub use aabb::Aabb;
pub use frustum::Frustum;
pub use plane::Plane;
pub use ray::{Cast, Hit, Ray};
pub use sphere::Sphere;
pub use triangle::Triangle;
