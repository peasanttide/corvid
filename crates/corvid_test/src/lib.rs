#![doc = include_str!("../README.md")]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the modules here are private, so pub(crate) and pub are equivalent — pub(crate) is the one that says what is meant, and it is what rustc's unreachable_pub asks for"
)]

// No `#![no_std]`. Three of the four checks here drive `corvid_app`, which opens
// directories and reads a clock, and the fourth walks a filesystem. This crate
// is a consumer of the platform ring rather than a member of the simulation
// one.

mod diverged;
mod goldens;
mod images;
mod replay;
mod reproducible;
mod roster;
mod scratchpad;

pub use diverged::{Diverged, Failed, What};
pub use goldens::{BLESS, EXTENSION, FLAVOUR, Finding, How, Mismatch, hex, matches_goldens, unhex};
pub use images::{Different, Frozen, Tolerance, images_agree, read_png};
pub use replay::replays_to_itself;
pub use reproducible::is_reproducible;
pub use scratchpad::Scratchpad;
