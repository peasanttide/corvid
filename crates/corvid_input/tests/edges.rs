//! Where an edge comes from, and what spends one.
//!
//! An edge is the whole reason `Devices` exists. A platform reports a key going
//! down and later reports it coming up, and a game reads `pressed` and
//! `released` for one frame -- so every test here is about an interval rather
//! than about a value, and the ones that would still pass if the edges were
//! computed by diffing two levels are marked as such.
//!
//! Focus and the captured control are here for the same reason: both are edges
//! the window raises rather than levels it reports.

#![cfg(feature = "platform")]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{action, snapshot, table};

use corvid_input::platform::{Axis, Bindings, Button, Devices, Key, MouseButton};
use corvid_input::{Analog, Digital, Input};

#[test]
fn a_tap_inside_one_frame_is_both_edges_and_no_hold() {
    // The case that rules out computing edges by comparing this frame's levels
    // against the last frame's: a key that went down and came up between two
    // snapshots is at the same level in both, so a diff sees nothing at all and
    // the player's tap is lost. It also rules out the near miss of recording
    // only the *last* event, which would report a release and no press.
    let mut devices = Devices::new();
    let mut input = snapshot();

    devices.press(Button::key(Key::Space));
    devices.release(Button::key(Key::Space));
    devices.snapshot(&table(), &mut input);

    assert_eq!(
        input.digital(action::NUDGE),
        Digital {
            held: false,
            pressed: true,
            released: true,
        },
    );
}

#[test]
fn an_edge_belongs_to_one_frame_and_a_level_belongs_to_every_frame() {
    // The two halves of what a snapshot consumes. A `pressed` that survived
    // into the next frame is an action that fires twice from one press; a
    // `held` that did not survive is a key the player is still holding that the
    // game thinks came up.
    let mut devices = Devices::new();
    let mut input = snapshot();

    devices.press(Button::key(Key::Space));
    devices.snapshot(&table(), &mut input);
    assert_eq!(
        input.digital(action::NUDGE),
        Digital {
            held: true,
            pressed: true,
            released: false,
        },
    );

    devices.snapshot(&table(), &mut input);
    assert_eq!(input.digital(action::NUDGE), Digital::HELD);
}

#[test]
fn a_repeat_from_the_platform_is_not_a_second_press() {
    // Every desktop re-reports a key that is being held down. A `pressed` that
    // believed it would be a menu that scrolls at the keyboard's repeat rate
    // rather than once, and the level would be right either way -- so this test
    // is about `pressed` alone and asserts the level too, to say that the key
    // really was still down.
    let mut devices = Devices::new();
    let mut input = snapshot();

    devices.press(Button::key(Key::Space));
    devices.snapshot(&table(), &mut input);

    devices.press(Button::key(Key::Space));
    devices.snapshot(&table(), &mut input);

    assert_eq!(input.digital(action::NUDGE), Digital::HELD);
}

#[test]
fn losing_focus_releases_what_was_held_and_drops_what_moved() {
    // The platform stops reporting releases the moment the player switches
    // away, so without this a key stays down for as long as they are gone. The
    // release edge is asserted as well as the level, because a game watching
    // for `released` is entitled to see one.
    let mut devices = Devices::new();
    let mut input = snapshot();

    devices.press(Button::key(Key::Space));
    devices.snapshot(&table(), &mut input);
    assert!(devices.is_held(Button::key(Key::Space)));

    devices.moved(Axis::MouseMotion, 100, 0);
    devices.released_all();
    devices.snapshot(&table(), &mut input);

    assert_eq!(
        input.digital(action::NUDGE),
        Digital {
            held: false,
            pressed: false,
            released: true,
        },
    );
    assert_eq!(input.delta(action::LOOK), Analog::ZERO);
}

#[test]
fn a_snapshot_reports_the_control_that_was_pressed_and_not_only_the_action() {
    // The route a rebinding screen asks its question through. Everything else
    // in a snapshot is an *action* -- a game declares what it can be asked to do
    // and never sees a key code -- and "press the control you want" is the one
    // question that cannot be asked that way.
    let mut devices = Devices::new();
    let mut input = Input::new(action::SETS);
    let bindings = Bindings::new().button(Button::key(Key::Space), action::NUDGE);

    devices.press(Button::key(Key::Space));
    devices.snapshot(&bindings, &mut input);
    assert_eq!(input.captured(), Some(Button::key(Key::Space)));
    // And the action it is bound to still fires, because this is beside the
    // table rather than instead of it.
    assert!(input.digital(action::NUDGE).pressed);
}

#[test]
fn a_control_bound_to_nothing_is_still_captured() {
    // The case that matters most: a player rebinding is most likely to reach
    // for a key that currently does nothing, and a capture that went through
    // the table would report nothing at all for exactly those.
    let mut devices = Devices::new();
    let mut input = Input::new(action::SETS);

    devices.press(Button::key(Key::J));
    devices.snapshot(&Bindings::new(), &mut input);
    assert_eq!(input.captured(), Some(Button::key(Key::J)));
}

#[test]
fn a_capture_is_an_edge_and_not_a_level() {
    // A press, and only on the frame of the press. A screen that bound on the
    // level would bind again on every frame the key stayed down, and one that
    // bound on release would bind whatever the player let go of.
    let mut devices = Devices::new();
    let mut input = Input::new(action::SETS);

    devices.press(Button::key(Key::K));
    devices.snapshot(&Bindings::new(), &mut input);
    assert_eq!(input.captured(), Some(Button::key(Key::K)));

    // Still held, and the next snapshot reports no capture.
    devices.snapshot(&Bindings::new(), &mut input);
    assert_eq!(input.captured(), None);

    devices.release(Button::key(Key::K));
    devices.snapshot(&Bindings::new(), &mut input);
    assert_eq!(input.captured(), None, "letting go is not a capture");
}

#[test]
fn two_controls_in_one_frame_report_the_same_one_on_every_machine() {
    // A frame that saw two presses has to answer with one of them, and which
    // one must not depend on the order the platform happened to mention them.
    let mut first = Devices::new();
    let mut second = Devices::new();
    first.press(Button::key(Key::A));
    first.press(Button::mouse(MouseButton::Left));
    second.press(Button::mouse(MouseButton::Left));
    second.press(Button::key(Key::A));

    let mut one = Input::new(action::SETS);
    let mut two = Input::new(action::SETS);
    first.snapshot(&Bindings::new(), &mut one);
    second.snapshot(&Bindings::new(), &mut two);
    assert_eq!(one.captured(), two.captured());
}

#[test]
fn a_capture_survives_a_fold_and_is_spent_by_a_settle() {
    // The same rule the edges follow, for the same reason: a loop that reads
    // its devices more often than it ticks folds the readings together, and a
    // press that happened between two ticks must not be dropped by a later
    // reading that saw nothing.
    let mut early = Input::new(action::SETS);
    early.set_captured(Some(Button::key(Key::L)));
    let quiet = Input::new(action::SETS);

    let mut folded = early.clone();
    folded.absorb(&quiet);
    assert_eq!(folded.captured(), Some(Button::key(Key::L)));

    folded.settle();
    assert_eq!(folded.captured(), None);
}

#[test]
fn focus_is_a_level_with_an_edge_either_side() {
    // A game that captures the pointer needs the *frame* the player came back
    // on, not merely that they are here now -- so this is a `Digital` and the
    // edges mean what they mean everywhere else.
    let mut devices = Devices::new();
    let mut input = Input::new(action::SETS);
    let bindings = Bindings::new();

    // A window nobody has told about is not focused, rather than assumed to be:
    // a run that started in the background must not take the pointer.
    devices.snapshot(&bindings, &mut input);
    assert_eq!(input.focus(), Digital::RELEASED);

    devices.focused(true);
    devices.snapshot(&bindings, &mut input);
    assert!(input.focus().held && input.focus().pressed);
    assert!(!input.focus().released);

    // Held, with no edge, on every frame after it.
    devices.snapshot(&bindings, &mut input);
    assert_eq!(input.focus(), Digital::HELD);

    devices.focused(false);
    devices.snapshot(&bindings, &mut input);
    assert!(input.focus().released && !input.focus().held);
}

#[test]
fn a_platform_repeating_itself_does_not_raise_an_edge() {
    // A platform reports focus far more often than it changes, and a game
    // taking the pointer back on every `pressed` would take it back on every
    // frame.
    let mut devices = Devices::new();
    let mut input = Input::new(action::SETS);
    let bindings = Bindings::new();

    devices.focused(true);
    devices.focused(true);
    devices.focused(true);
    devices.snapshot(&bindings, &mut input);
    assert!(input.focus().pressed);

    devices.focused(true);
    devices.snapshot(&bindings, &mut input);
    assert!(
        !input.focus().pressed,
        "focus was regained without being lost"
    );
}

#[test]
fn focus_is_not_filtered_by_the_active_set() {
    // It is a property of the window rather than something a player did, so
    // there is nothing to bind it to and no set that could silence it.
    let mut devices = Devices::new();
    let mut input = Input::new(action::SETS);
    devices.focused(true);
    devices.snapshot(&Bindings::new(), &mut input);
    input.activate(action::Paused::ID);
    assert!(input.focus().held);
}

#[test]
fn losing_focus_reports_the_leaving_as_well_as_releasing_what_was_held() {
    // The two halves of the same event. Everything held goes up, because the
    // platform stops reporting releases -- and the *leaving itself* is reported,
    // because no key release says "the player left" and a game that captured
    // the pointer has to know.
    let mut devices = Devices::new();
    let mut input = Input::new(action::SETS);
    let bindings = Bindings::new().button(Button::key(Key::Space), action::NUDGE);

    devices.focused(true);
    devices.press(Button::key(Key::Space));
    devices.snapshot(&bindings, &mut input);
    assert!(input.digital(action::NUDGE).held);

    devices.released_all();
    devices.focused(false);
    devices.snapshot(&bindings, &mut input);
    assert!(input.digital(action::NUDGE).released);
    assert!(input.focus().released);
}
