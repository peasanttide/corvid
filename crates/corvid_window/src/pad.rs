//! Gamepads, which `winit` does not read.
//!
//! `winit` owns windows and the devices a window has — a keyboard and a mouse —
//! and a pad is neither: it is a device the operating system exposes on its own
//! terms, through `evdev` on Linux, `XInput` on Windows and `IOKit` on macOS.
//! `gilrs` is the crate that covers those three, and this module is the whole of
//! what this workspace knows about it.
//!
//! # The seam
//!
//! Everything below turns `gilrs`'s events into the vocabulary
//! [`corvid_input::platform`] already had — a [`Button`], an [`Axis`], a
//! deflection — and hands them to [`Devices`], which is the same accumulator a
//! key goes through. Nothing downstream of that line can tell a pad from a
//! keyboard: the union that lets two controls drive one action is the same
//! union, a binding file names a pad button the same way it names a key, and the
//! rebinding screen captures one the same way.
//!
//! That is why this file is small, and why it is the only file in the workspace
//! that names `gilrs`. Swapping the backend is this module; adding a pad to a
//! game is a line in a binding table.
//!
//! # What it does not do
//!
//! **Rumble, and per-pad identity.** [`Button`] carries no pad number, so every
//! pad the platform reports folds into one set of controls: two pads are two
//! hands on one seat rather than two players. Splitting them needs a map from
//! device to seat, which is a design a local-multiplayer game would make rather
//! than inherit.

use corvid_input::platform::{Axis, Button, Devices, PadButton};

/// How far a stick must be pushed before it counts as pushed at all.
///
/// A stick at rest does not report zero. Wear, drift and manufacturing leave a
/// resting position a little off centre, and a game that took the reading
/// literally turns slowly for ever while nobody is touching it — which reads as
/// a haunted camera and is the single most common complaint about pad support.
///
/// A sixteenth of full deflection. Small enough not to eat the beginning of a
/// slow push and large enough to cover a tired stick.
const DEADZONE: f32 = 1.0 / 16.0;

/// What a full deflection is worth in the device units a binding divides.
///
/// A binding's span is an integer, and `gilrs` reports a float in `-1.0 ..=
/// 1.0`, so this is the scale between them. A table binding a stick sets its
/// span to this number and gets a full sweep for a full push — which is what
/// `hello`'s `STICK_SPAN` is, stated on the game's side of the line.
///
/// An `f32` because that is what it is multiplied by, and 32 767 is exactly
/// representable there — a `u32` cast at the point of use is a precision
/// warning about a number that has none to lose.
const FULL: f32 = 32_767.0;

/// Every pad on the machine, polled once per frame.
///
/// A thin wrapper: what it owns is `gilrs`'s own context, and what it does is
/// drain the queue into [`Devices`].
pub(crate) struct Pads {
    /// The library's context, or [`None`] if it would not start.
    ///
    /// A machine with no permission to read input devices — a container, a
    /// locked-down desktop — is a machine that plays with a keyboard, not a
    /// machine that fails to open a window. The failure is reported once and
    /// then never mentioned again.
    context: Option<gilrs::Gilrs>,
}

impl Pads {
    /// Opens the pad backend, reporting rather than failing.
    #[must_use]
    pub(crate) fn new() -> Self {
        match gilrs::Gilrs::new() {
            Ok(context) => {
                let pads = context.gamepads().count();
                tracing::info!(
                    name: "corvid_window.pads",
                    pads,
                    "the gamepad backend is open",
                );
                Self {
                    context: Some(context),
                }
            }
            Err(why) => {
                tracing::warn!(
                    name: "corvid_window.no_pads",
                    error = %why,
                    "no gamepad backend on this machine, so pads do nothing and \
                     everything else is unaffected",
                );
                Self { context: None }
            }
        }
    }

    /// Drains everything the pads have done into `devices`.
    ///
    /// Called once per frame, before the snapshot is taken, for the same reason
    /// the accumulated mouse motion is paid there: what a frame reports is what
    /// happened during it.
    pub(crate) fn pump(&mut self, devices: &mut Devices) {
        let Some(context) = self.context.as_mut() else {
            return;
        };
        while let Some(event) = context.next_event() {
            match event.event {
                gilrs::EventType::ButtonPressed(button, _) => {
                    if let Some(button) = press(button) {
                        devices.press(button);
                    }
                }
                gilrs::EventType::ButtonReleased(button, _) => {
                    if let Some(button) = press(button) {
                        devices.release(button);
                    }
                }
                // A pad unplugged mid-game leaves whatever it was holding down
                // held for ever, which is the same failure a window losing
                // focus has and wants the same answer. The keyboard's held
                // buttons go with it, which is acceptable: a player who just
                // unplugged a pad is not mid-keystroke.
                gilrs::EventType::Disconnected => {
                    tracing::info!(name: "corvid_window.pad_left", "a pad was unplugged");
                    devices.released_all();
                }
                gilrs::EventType::Connected => {
                    tracing::info!(name: "corvid_window.pad_arrived", "a pad was plugged in");
                }
                _ => {}
            }
        }

        // The sticks and triggers are read as *levels* rather than from the
        // event queue, because that is what they are: a stick held over is
        // still held over on a frame nobody reported anything about it, and
        // `Devices::deflected` is the accessor that survives a snapshot.
        //
        // The last pad to report wins where several are plugged in, which is
        // the same folding `Button` does and is documented at the top of this
        // file.
        for (_, pad) in context.gamepads() {
            devices.deflected(
                Axis::LeftStick,
                scaled(pad.value(gilrs::Axis::LeftStickX)),
                scaled(pad.value(gilrs::Axis::LeftStickY)),
            );
            devices.deflected(
                Axis::RightStick,
                scaled(pad.value(gilrs::Axis::RightStickX)),
                scaled(pad.value(gilrs::Axis::RightStickY)),
            );
            // Triggers rest at zero and only go one way, so no deadzone is
            // applied to them: what a deadzone is for is a control that is
            // supposed to be centred and is not, and a trigger is supposed to
            // be at rest.
            devices.deflected(
                Axis::Triggers,
                whole(pad.value(gilrs::Axis::LeftZ)),
                whole(pad.value(gilrs::Axis::RightZ)),
            );
        }
    }
}

impl Default for Pads {
    fn default() -> Self {
        Self::new()
    }
}

/// A stick reading in device units, with the deadzone taken out.
///
/// Rescaled rather than clamped: a stick just past the deadzone reads as *just*
/// pushed rather than jumping to a sixteenth, so the control is continuous
/// across the boundary. Clamping is what makes a camera start with a lurch.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the value is clamped into ±1 before it is scaled by FULL, so the product is inside an i32 by construction"
)]
fn scaled(value: f32) -> i32 {
    let magnitude = value.abs();
    if magnitude <= DEADZONE {
        return 0;
    }
    let past = (magnitude - DEADZONE) / (1.0 - DEADZONE);
    let scaled = past.clamp(0.0, 1.0) * value.signum() * FULL;
    scaled as i32
}

/// A trigger reading in device units, with no deadzone.
#[allow(
    clippy::cast_possible_truncation,
    reason = "clamped into ±1 before scaling, as above"
)]
fn whole(value: f32) -> i32 {
    (value.clamp(-1.0, 1.0) * FULL) as i32
}

/// The button a `gilrs` button denotes, or [`None`] for one this vocabulary
/// does not name.
///
/// The face buttons are mapped **by position and not by letter**, which is the
/// whole reason [`PadButton`] is spelled the way it is: `gilrs::Button::South`
/// is the bottom face button on every pad it supports, whatever is printed on
/// it.
const fn press(button: gilrs::Button) -> Option<Button> {
    let button = match button {
        gilrs::Button::South => PadButton::South,
        gilrs::Button::East => PadButton::East,
        gilrs::Button::West => PadButton::West,
        gilrs::Button::North => PadButton::North,
        gilrs::Button::LeftTrigger => PadButton::LeftBumper,
        gilrs::Button::RightTrigger => PadButton::RightBumper,
        gilrs::Button::LeftTrigger2 => PadButton::LeftTrigger,
        gilrs::Button::RightTrigger2 => PadButton::RightTrigger,
        gilrs::Button::Select => PadButton::Select,
        gilrs::Button::Start => PadButton::Start,
        gilrs::Button::Mode => PadButton::Guide,
        gilrs::Button::LeftThumb => PadButton::LeftStick,
        gilrs::Button::RightThumb => PadButton::RightStick,
        gilrs::Button::DPadUp => PadButton::PadUp,
        gilrs::Button::DPadDown => PadButton::PadDown,
        gilrs::Button::DPadLeft => PadButton::PadLeft,
        gilrs::Button::DPadRight => PadButton::PadRight,
        // `Unknown`, and anything a later `gilrs` adds. Dropped rather than
        // given a number, because the number would be `gilrs`'s and a binding
        // file naming it would not survive that crate being replaced.
        _ => return None,
    };
    Some(Button::pad(button))
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "these compare integers derived from floats, and the boundaries are exact"
)]
mod tests {
    use super::*;

    #[test]
    fn a_stick_at_rest_reads_exactly_zero() {
        // The haunted camera, as an assertion. A worn stick resting a little
        // off centre must read as centred rather than as a slow permanent turn.
        assert_eq!(scaled(0.0), 0);
        assert_eq!(scaled(DEADZONE), 0);
        assert_eq!(scaled(-DEADZONE), 0);
        assert_eq!(scaled(DEADZONE * 0.9), 0);
    }

    #[test]
    fn the_deadzone_is_rescaled_rather_than_clamped() {
        // Just past the edge reads as just pushed, rather than jumping to a
        // sixteenth of full deflection — which is a camera that starts with a
        // lurch.
        let past = scaled(DEADZONE * 1.001);
        assert!(
            past > 0 && past < 100,
            "a nudge past the deadzone read {past}"
        );
    }

    #[test]
    fn a_full_push_is_a_full_deflection_either_way() {
        let full = 32_767;
        assert_eq!(scaled(1.0), full);
        assert_eq!(scaled(-1.0), -full);
        // And past full, which a badly calibrated pad reports.
        assert_eq!(scaled(1.5), full);
    }

    #[test]
    fn a_trigger_has_no_deadzone_and_rests_at_zero() {
        assert_eq!(whole(0.0), 0);
        assert_eq!(whole(1.0), 32_767);
        // A trigger barely touched reports something, unlike a stick barely
        // moved: it is supposed to be at rest, so leaving rest is real.
        assert!(whole(0.01) > 0);
    }

    #[test]
    fn the_face_buttons_are_mapped_by_position() {
        // The claim `PadButton` is built on: south is the bottom face button,
        // whatever letter is printed on it.
        assert_eq!(
            press(gilrs::Button::South),
            Some(Button::pad(PadButton::South)),
        );
        assert_eq!(
            press(gilrs::Button::North),
            Some(Button::pad(PadButton::North)),
        );
        // And the two shoulder rows do not get crossed: `gilrs` calls the
        // bumper `LeftTrigger` and the trigger `LeftTrigger2`, which is exactly
        // the sort of thing a mapping table exists to get right once.
        assert_eq!(
            press(gilrs::Button::LeftTrigger),
            Some(Button::pad(PadButton::LeftBumper)),
        );
        assert_eq!(
            press(gilrs::Button::LeftTrigger2),
            Some(Button::pad(PadButton::LeftTrigger)),
        );
        assert_eq!(press(gilrs::Button::Unknown), None);
    }
}
