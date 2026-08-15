#![doc = include_str!("../README.md")]

// No `#![no_std]`. Three of the four checks here drive `corvid_app`, which opens
// directories and reads a clock, and the fourth walks a filesystem. This crate
// is a consumer of the platform ring rather than a member of the simulation
// one.

mod diverged;
mod goldens;
mod images;
mod mismatch;
mod replay;
mod reproducible;
mod roster;
mod scratchpad;

pub use diverged::{Diverged, Failed, What};
pub use goldens::{BLESS, EXTENSION, hex, matches_goldens, unhex};
pub use images::{Different, Frozen, Tolerance, images_agree, read_png};
pub use mismatch::{Finding, How, Mismatch};
pub use replay::replays_to_itself;
pub use reproducible::is_reproducible;
pub use scratchpad::Scratchpad;
