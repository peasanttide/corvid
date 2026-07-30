//! The four families and the macros that generate them.
//!
//! Each family module owns the macro that knows its arithmetic; the shared
//! generators live in [`macros`]. The types themselves are re-exported from the
//! crate root, which is where they should be referred to from.

mod macros;

pub mod angle;
pub mod factor;
pub mod point;
pub mod signed;
