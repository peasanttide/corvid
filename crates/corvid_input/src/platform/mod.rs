//! The half that faces devices: a control vocabulary, a binding table, and the
//! accumulator that turns a stream of platform events into one [`Input`].
//!
//! [`Input`]: crate::Input
//!
//! The default half of this crate is what a game reads. This half is what fills
//! it in, and the two are separated because [`Present::intend`] must stay in
//! the client ring: a game turning a snapshot into an action may not need an
//! operating system, and the code reading a keyboard obviously does.
//!
//! [`Present::intend`]: https://docs.rs/corvid_present
//!
//! # What is here, and what is not
//!
//! Here: the [`Bindings`] table between a device-neutral control and a game's
//! declared actions, the [`Reading`] that says whether a binding reports a
//! deflection or a displacement, [`Devices`], which accumulates events and
//! hands back a snapshot with its edges worked out, and [`Table`], which is a
//! binding table written down by name.
//!
//! The vocabulary those are written in — [`Button`], [`Axis`], [`Key`] and
//! [`MouseButton`] — is re-exported here and *defined* outside this feature,
//! for two reasons. A key names no device driver and needs no operating system,
//! so nothing about it belongs behind a gate that means "faces devices". And
//! [`Input::captured`](crate::Input::captured) carries one, so a rebinding
//! screen can ask which control the player just pressed without turning on the
//! half that reads a keyboard.
//!
//! [`Bindings`] itself is named by [`Present`], which is why this feature is on
//! unconditionally for `corvid_present`: a game *authoring* a table is stating
//! data, and no part of it reads a device. [`Devices`], which does, is named by
//! `corvid_window` and by nothing in the client ring.
//!
//! [`Present`]: https://docs.rs/corvid_present
//!
//! Not here: hot-plug, glyph lookup, rumble, a rebinding screen, gamepads, and
//! any per-device default table. [`Bindings::placeholder`] stands in for the
//! last of them and says so at length.
//!
//! This module names no windowing library. Translating a platform's own events
//! into a [`Button`] or an [`Axis`] is the event loop's job — `corvid_window`
//! does it for `winit` — which is what lets this half stay `no_std` and lets a
//! binding file be written down without a window anywhere near it.

mod bind;
mod devices;
// The by-name table is what a file holds, and a file is written by `serde`.
// Nothing else here needs it, so nothing else here is gated on it.
#[cfg(feature = "serde")]
mod table;

pub use bind::{AxisBinding, Bindings, Component, PairBinding, Reading};
pub use devices::Devices;
#[cfg(feature = "serde")]
pub use table::{Table, Unknown};

// The vocabulary lives outside this feature and is re-exported here, because a
// `Key` names no device and needs no operating system, and `Input::captured`
// carries a `Button` for a rebinding screen to read. These paths are the ones a
// binding was always written against, so they stay.
pub use crate::source::{Axis, Button, Key, MouseButton, PadButton};
