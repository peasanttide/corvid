#![doc = include_str!("../README.md")]
#![no_std]

// A pool of particles and a table of emitters both grow, so `alloc` is the one
// thing past `core` this crate needs -- and the whole of what it needs, which is
// why it builds for a target with no operating system.
extern crate alloc;

mod emitter;
mod error;
mod instance;
mod motion;
mod particle;
mod ramp;
mod rng;
mod shape;
mod system;
mod table;
mod vector;

pub use emitter::{Emitter, EmitterId, Trail};
pub use error::ParticleError;
pub use instance::Instance;
pub use ramp::{ColorRamp, Range};
pub use rng::Rng;
pub use shape::Shape;
pub use system::System;
