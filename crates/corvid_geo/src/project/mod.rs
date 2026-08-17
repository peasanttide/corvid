//! The bake-time half: floating point, `std`, and never a tick.
//!
//! Everything in here computes with transcendental functions in `f64`, which
//! is the one thing the rest of this crate exists to keep out of a simulation.
//! A map projection genuinely needs a logarithm and a power, and a closed-form
//! geodetic inverse genuinely needs a cube root, so the answer is not to
//! approximate them badly in integers -- it is to run them once, when a level
//! is built, and store what came out as fixed point.
//!
//! The seam is [`Wgs84::to_geodetic`]. Above it, degrees and `f64`. Below it,
//! [`Geodetic`](crate::Geodetic) and integers, and a hash that means the same
//! thing on every machine.

mod conic;
mod geodesy;

pub use conic::{ConformalConic, Projected};
pub use geodesy::Wgs84;
