//! The builder prints, and keeps printing.
//!
//! [`App`](corvid_app::App) derives [`Debug`], and a derive's bounds are
//! implicit: it asks for `Debug` on the type parameters rather than on the
//! field types, and it is satisfied or not according to what the fields happen
//! to be. So a field added later that is not `Debug` does not fail to
//! compile — it quietly removes the impl, and every `{:?}` on an `App`
//! elsewhere starts failing to resolve instead.
//!
//! This is the assertion that turns that into a failure here.

mod common;

use common::{Ears, Hands, Painted, Tally};
use corvid_app::App;

/// Compiles only while `T` is [`Debug`].
const fn printable<T: std::fmt::Debug>() {}

#[test]
fn the_builder_can_be_printed() {
    printable::<App<Tally, Hands, Painted, Ears>>();
}
