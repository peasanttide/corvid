#![doc = include_str!("../README.md")]
// No `#![no_std]`. This is tooling: it formats, it sorts, and it sits beside a
// runtime that already opens files.

mod argument;
mod console;
mod inspect;
mod overlay;
mod slider;
mod tune;

pub use argument::{Argument, Arguments, Invalid, Parameter};
pub use console::{Completion, Console, Entry, Handler, HelpLine, Registered, Reply};
pub use inspect::{Group, Inspect, Row, Rows};
pub use overlay::{Overlay, dump_audio};
pub use slider::Slider;
pub use tune::{Proposal, Tunable, Tuning};
