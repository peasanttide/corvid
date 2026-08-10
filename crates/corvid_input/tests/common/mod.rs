//! The declaration and the table the three platform suites bind against.
//!
//! Shared because three test binaries read the same fixture, and a fixture
//! copied between them is a fixture that drifts: the numbering these
//! declarations hand out is itself under test in `tests/golden.rs`, so two
//! copies that disagreed would be two suites testing different crates.

#![allow(
    unreachable_pub,
    dead_code,
    reason = "each of the three suites includes this module and uses a subset of it"
)]

use core::num::NonZeroU32;

use corvid_fixed::Signed16;
use corvid_input::Input;
use corvid_input::platform::{Axis, Bindings, Button, Key, MouseButton, Reading};

/// Half a span, as the axis a half-span reading lands on.
///
/// An axis is `bits / 32767` and 32767 is odd, so half of one is an exact tie
/// between two neighbouring axes -- the only tie the scaling can actually be
/// handed -- and `Signed16::saturating_from_ratio` rounds it away from zero.
/// Named rather than spelled out at six call sites, because `16_384` at a
/// glance looks like a wrong `16_383` and the tie is the whole reason it is
/// not.
pub const HALF_SPAN: Signed16 = Signed16::from_bits(16_384);

/// Two sets, so that the "an inactive set answers with nothing" property has
/// somewhere to be tested.
pub mod action {
    corvid_input::action_sets! {
        pub set Playing {
            digital NUDGE, FIRE;
            analog LOOK;
        }
        pub set Paused {
            digital RESUME;
        }
    }
}

/// A span of one hundred device units, which is a round number to halve.
pub const fn span(units: u32) -> NonZeroU32 {
    match NonZeroU32::new(units) {
        Some(span) => span,
        None => NonZeroU32::MIN,
    }
}

/// A declaration with more digital actions than there are placeholder keys.
///
/// At the top of the file rather than inside the test that uses it, because an
/// item after a statement is an item whose scope is not where it is written.
pub mod many {
    corvid_input::action_sets! {
        pub set Everything {
            digital A0, A1, A2, A3, A4, A5, A6, A7, A8, A9,
                    B0, B1, B2, B3, B4, B5, B6, B7, B8, B9;
        }
    }
}

/// The table every test here binds against unless it says otherwise.
pub fn table() -> Bindings {
    Bindings::new()
        .button(Button::key(Key::Space), action::NUDGE)
        .button(Button::mouse(MouseButton::Left), action::FIRE)
        .axis(
            Axis::MouseMotion,
            action::LOOK,
            span(100),
            Reading::Displacement,
        )
}

/// A snapshot over the declaration above.
pub fn snapshot() -> Input {
    Input::new(action::SETS)
}
