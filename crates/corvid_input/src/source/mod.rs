//! The device-neutral vocabulary a binding is written in.
//!
//! Nothing here names a windowing library, a scancode table or an operating
//! system. A key is named by where it sits on the board rather than by what is
//! printed on it, so a binding to [`Key::W`] is the same physical key on a
//! QWERTY board and on an AZERTY one -- which is what a player who moves with
//! the three keys next to `A` actually means.
//!
//! Translating a platform's own event into one of these is the job of whoever
//! owns the event loop; `corvid_window` is the crate that does it for `winit`.
//!
//! Every type here writes itself down and reads itself back, because a binding
//! file names controls in text and a rebinding screen shows them in text, and
//! neither should keep a table of its own to do it.
//!
//! Split three ways because a file stays under 400 lines, and the seams were
//! already there: a key is generated from a table, a button is an enum of three
//! device kinds, and an axis is the only one of the three that carries a
//! quantity rather than a name.

mod axis;
mod button;
mod key;

pub use axis::Axis;
pub use button::{Button, MouseButton, PadButton};
pub use key::Key;
