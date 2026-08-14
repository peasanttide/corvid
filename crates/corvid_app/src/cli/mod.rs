//! The command line: what a Corvid game answers to, how it is read, and the one
//! `main` that acts on it.
//!
//! The seam between the four files under this one is the process. `entry.rs`
//! is the only one that exits it or writes to its streams -- the usage and the
//! digest on stdout, a refused command line and a run that could not finish on
//! stderr, which are the only writes in the crate and are all in sight of each
//! other. `arguments.rs` parses, `argument.rs` is what parsing refuses with,
//! and `watch.rs` is the subscriber a binary may install.

// `crate::Result` is spelled in full at each use below rather than imported:
// this file also parses into `Result<_, Argument>`, and one `Result` in scope
// standing for a one-parameter alias would shadow the other.

mod argument;
mod arguments;
mod entry;
mod watch;

pub use argument::Argument;
pub use arguments::{Arguments, Load};
pub use entry::main;
pub use watch::watch;
