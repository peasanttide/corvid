//! `winit`'s vocabulary turned into `corvid_input`'s.
//!
//! This is the only module in the workspace that names a `winit` key code, and
//! that is the point: `corvid_input`'s [`Key`] is device-neutral and `no_std`,
//! so a binding file, a rebinding screen and a replay of an input log never
//! learn which windowing library the frame came from.

use corvid_input::platform::{Button, Key, MouseButton};
use winit::event::MouseButton as WinitButton;
use winit::keyboard::KeyCode;

/// The [`Key`] a physical key code means, or [`None`] for one this vocabulary
/// does not name.
///
/// A key that is not named cannot be bound, so returning `None` is how a media
/// key or a numeric pad is left out rather than mapped onto something it is
/// not. `corvid_input::platform::Key` is what would grow a variant.
pub(crate) const fn key(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::KeyA => Key::A,
        KeyCode::KeyB => Key::B,
        KeyCode::KeyC => Key::C,
        KeyCode::KeyD => Key::D,
        KeyCode::KeyE => Key::E,
        KeyCode::KeyF => Key::F,
        KeyCode::KeyG => Key::G,
        KeyCode::KeyH => Key::H,
        KeyCode::KeyI => Key::I,
        KeyCode::KeyJ => Key::J,
        KeyCode::KeyK => Key::K,
        KeyCode::KeyL => Key::L,
        KeyCode::KeyM => Key::M,
        KeyCode::KeyN => Key::N,
        KeyCode::KeyO => Key::O,
        KeyCode::KeyP => Key::P,
        KeyCode::KeyQ => Key::Q,
        KeyCode::KeyR => Key::R,
        KeyCode::KeyS => Key::S,
        KeyCode::KeyT => Key::T,
        KeyCode::KeyU => Key::U,
        KeyCode::KeyV => Key::V,
        KeyCode::KeyW => Key::W,
        KeyCode::KeyX => Key::X,
        KeyCode::KeyY => Key::Y,
        KeyCode::KeyZ => Key::Z,
        KeyCode::Digit0 => Key::Digit0,
        KeyCode::Digit1 => Key::Digit1,
        KeyCode::Digit2 => Key::Digit2,
        KeyCode::Digit3 => Key::Digit3,
        KeyCode::Digit4 => Key::Digit4,
        KeyCode::Digit5 => Key::Digit5,
        KeyCode::Digit6 => Key::Digit6,
        KeyCode::Digit7 => Key::Digit7,
        KeyCode::Digit8 => Key::Digit8,
        KeyCode::Digit9 => Key::Digit9,
        KeyCode::Space => Key::Space,
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::Escape => Key::Escape,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::ArrowDown => Key::ArrowDown,
        KeyCode::ArrowLeft => Key::ArrowLeft,
        KeyCode::ArrowRight => Key::ArrowRight,
        KeyCode::ShiftLeft => Key::LeftShift,
        KeyCode::ShiftRight => Key::RightShift,
        KeyCode::ControlLeft => Key::LeftControl,
        KeyCode::ControlRight => Key::RightControl,
        KeyCode::AltLeft => Key::LeftAlt,
        KeyCode::AltRight => Key::RightAlt,
        _ => return None,
    })
}

/// The [`Button`] a mouse button means.
///
/// `winit`'s `Back` and `Forward` become numbered buttons rather than names of
/// their own, because what a side button means is the player's business and a
/// binding table is where it is decided.
pub(crate) const fn mouse(button: WinitButton) -> Button {
    Button::Mouse(match button {
        WinitButton::Left => MouseButton::Left,
        WinitButton::Right => MouseButton::Right,
        WinitButton::Middle => MouseButton::Middle,
        WinitButton::Back => MouseButton::Other(3),
        WinitButton::Forward => MouseButton::Other(4),
        WinitButton::Other(number) => MouseButton::Other(number),
    })
}

#[cfg(test)]
mod tests {
    //! Whether every key a game can bind is a key a board can produce.

    #![allow(
        clippy::panic,
        reason = "a failed assertion in a test is a failed test, which is what a test is for"
    )]

    use std::collections::BTreeSet;

    use super::{Button, Key, KeyCode, MouseButton, WinitButton, key, mouse};

    /// Every physical key code this crate claims to translate.
    ///
    /// Listed rather than derived, because `winit` has no enumeration of its
    /// own to walk. What the test below does with it is not a re-statement of
    /// the match arms: it asks whether the *image* of this list is the whole of
    /// `Key::ALL`, which is what catches a `Key` variant that nothing on a
    /// keyboard can produce — an action bound to it would never fire and
    /// nothing else would notice.
    const CLAIMED: &[KeyCode] = &[
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Space,
        KeyCode::Enter,
        KeyCode::Escape,
        KeyCode::Tab,
        KeyCode::Backspace,
        KeyCode::ArrowUp,
        KeyCode::ArrowDown,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
    ];

    #[test]
    fn every_key_a_game_can_bind_is_one_a_board_can_produce() {
        let reachable: BTreeSet<Key> = CLAIMED.iter().filter_map(|&code| key(code)).collect();
        let declared: BTreeSet<Key> = Key::ALL.iter().copied().collect();
        assert_eq!(
            reachable, declared,
            "a key exists that no physical key produces, or the other way round",
        );
    }

    #[test]
    fn a_key_this_vocabulary_does_not_name_is_left_alone() {
        // Returning `None` rather than mapping onto something nearby: an F-key
        // silently bound to `Escape` would close the game.
        assert_eq!(key(KeyCode::F1), None);
        assert_eq!(key(KeyCode::Numpad0), None);
        // And a spot check that the translation is a translation rather than an
        // ordering coincidence, since the test above would pass on any
        // bijection at all.
        assert_eq!(key(KeyCode::KeyW), Some(Key::W));
        assert_eq!(key(KeyCode::Escape), Some(Key::Escape));
        assert_eq!(key(KeyCode::ShiftRight), Some(Key::RightShift));
    }

    #[test]
    fn the_numeric_pad_enter_is_the_same_action_as_the_other_one() {
        // Deliberately not injective. A player who presses either expects the
        // same thing, and a binding screen that made them separate would be
        // asking about a distinction nobody has.
        assert_eq!(key(KeyCode::NumpadEnter), key(KeyCode::Enter));
    }

    #[test]
    fn the_side_buttons_are_numbered_rather_than_named() {
        assert_eq!(mouse(WinitButton::Left), Button::Mouse(MouseButton::Left));
        assert_ne!(mouse(WinitButton::Back), mouse(WinitButton::Forward));
        assert_eq!(
            mouse(WinitButton::Other(7)),
            Button::Mouse(MouseButton::Other(7)),
        );
    }
}
