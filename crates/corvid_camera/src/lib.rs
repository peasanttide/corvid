#![doc = include_str!("../README.md")]
#![no_std]

// No `extern crate alloc`. A camera is a pose, a frustum and three numbers.

mod camera;
mod eye;
pub mod matrix;
mod orbit;
mod person;

pub use camera::Camera;
pub use eye::Eye;
pub use orbit::Orbit;
pub use person::FirstPerson;
