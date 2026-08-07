//! The half that faces devices: what a binding table is, and where an edge
//! comes from.
//!
//! An edge is the whole reason [`Devices`] exists. A platform reports a key
//! going down and later reports it coming up, and a game reads `pressed` and
//! `released` for one frame — so every test below that names an edge is about
//! an interval rather than about a value, and the ones that would still pass if
//! the edges were computed by diffing two levels are marked as such.

#![cfg(feature = "platform")]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

use core::num::NonZeroU32;

use corvid_fixed::Signed16;
use corvid_input::platform::{
    Axis, Bindings, Button, Devices, Key, MouseButton, PadButton, Reading,
};
use corvid_input::{Analog, AnalogId, Digital, DigitalId, Input};
/// Two sets, so that the "an inactive set answers with nothing" property has
/// somewhere to be tested.
mod action {
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
const fn span(units: u32) -> NonZeroU32 {
    match NonZeroU32::new(units) {
        Some(span) => span,
        None => NonZeroU32::MIN,
    }
}

/// A declaration with more digital actions than there are placeholder keys.
///
/// At the top of the file rather than inside the test that uses it, because an
/// item after a statement is an item whose scope is not where it is written.
mod many {
    corvid_input::action_sets! {
        pub set Everything {
            digital A0, A1, A2, A3, A4, A5, A6, A7, A8, A9,
                    B0, B1, B2, B3, B4, B5, B6, B7, B8, B9;
        }
    }
}

/// The table every test here binds against unless it says otherwise.
fn table() -> Bindings {
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
fn snapshot() -> Input {
    Input::new(action::SETS)
}

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
    // rather than once, and the level would be right either way — so this test
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
fn several_buttons_on_one_action_are_the_union() {
    // Two controls, one action, and the second binding must not overwrite what
    // the first said. A table that assigned rather than merged would report the
    // action released whenever the *last* binding for it was up, which is the
    // common shape of this bug and is invisible whenever both are down.
    let both = Bindings::new()
        .button(Button::key(Key::LeftShift), action::NUDGE)
        .button(Button::key(Key::RightShift), action::NUDGE);
    let mut devices = Devices::new();
    let mut input = snapshot();

    devices.press(Button::key(Key::LeftShift));
    devices.snapshot(&both, &mut input);
    assert!(input.digital(action::NUDGE).held, "the first binding");

    devices.release(Button::key(Key::LeftShift));
    devices.press(Button::key(Key::RightShift));
    devices.snapshot(&both, &mut input);
    assert!(input.digital(action::NUDGE).held, "the second binding");
}

#[test]
fn several_axes_on_one_action_are_the_union_too() {
    // The analog half of the test above. An axis loop that assigned where the
    // button loop merges would let the *last* binding for an action write its
    // own reading over whatever the others said — and a control nobody is
    // moving reads zero, so binding a wheel to look would silently stop the
    // mouse from looking. Both orders are exercised below, because a table that
    // answers correctly one way round and not the other has that bug.
    let both = Bindings::new()
        .axis(
            Axis::MouseMotion,
            action::LOOK,
            span(100),
            Reading::Displacement,
        )
        .axis(Axis::Scroll, action::LOOK, span(100), Reading::Displacement);
    let mut devices = Devices::new();
    let mut input = snapshot();

    // The first binding moves and the second is still.
    devices.moved(Axis::MouseMotion, 50, 0);
    devices.snapshot(&both, &mut input);
    assert_eq!(
        input.delta(action::LOOK).x,
        Signed16::from_bits(16_383),
        "the quiet scroll binding zeroed what the mouse reported",
    );

    // And the other way round, so the answer does not depend on which row is
    // last.
    devices.moved(Axis::Scroll, 50, 0);
    devices.snapshot(&both, &mut input);
    assert_eq!(
        input.delta(action::LOOK).x,
        Signed16::from_bits(16_383),
        "the quiet mouse binding zeroed what the scroll reported",
    );

    // Neither moving is still zero, which is what says the union is a union
    // rather than a value that sticks.
    devices.snapshot(&both, &mut input);
    assert_eq!(input.delta(action::LOOK), Analog::ZERO);
}

#[test]
fn motion_is_a_delta_and_the_pointer_is_a_level() {
    // A mouse that keeps turning the camera after the player stopped moving it
    // is the accumulated motion outliving the snapshot that read it. A pointer
    // that vanishes the frame after it arrived is the opposite mistake, and one
    // fix for either of them is usually the other bug.
    let mut devices = Devices::new();
    let mut input = snapshot();
    let where_it_is = Analog::new(Signed16::from_f64(0.25), Signed16::from_f64(-0.5));

    devices.moved(Axis::MouseMotion, 50, 0);
    devices.point(Some(where_it_is));
    devices.snapshot(&table(), &mut input);
    assert_ne!(input.delta(action::LOOK), Analog::ZERO);
    assert_eq!(input.pointer(), Some(where_it_is));

    devices.snapshot(&table(), &mut input);
    assert_eq!(input.delta(action::LOOK), Analog::ZERO);
    assert_eq!(input.pointer(), Some(where_it_is));
}

#[test]
fn motion_is_scaled_by_the_span_and_clamped_at_it() {
    // Three points on one curve, because any one of them alone is satisfied by
    // a constant. The endpoint matters most: a full span must be exactly
    // `Signed16::MAX` rather than one short of it, since a full sweep is what
    // an analog action's whole range is defined against.
    let mut devices = Devices::new();
    let mut input = snapshot();
    let table = table();

    devices.moved(Axis::MouseMotion, 100, 0);
    devices.snapshot(&table, &mut input);
    assert_eq!(input.delta(action::LOOK).x, Signed16::MAX);

    devices.moved(Axis::MouseMotion, 50, 0);
    devices.snapshot(&table, &mut input);
    assert_eq!(input.delta(action::LOOK).x, Signed16::from_bits(16_383));

    devices.moved(Axis::MouseMotion, -100_000, 0);
    devices.snapshot(&table, &mut input);
    assert_eq!(input.delta(action::LOOK).x, Signed16::MIN);
}

#[test]
fn a_displacement_binding_answers_on_delta_and_a_deflection_binding_on_analog() {
    // The split, as the one property that makes reaching for the wrong
    // accessor a value that stays still. Both directions are asserted, because
    // a `snapshot` that wrote both accessors from one reading would satisfy
    // either half on its own — and so would one that wrote neither, which is
    // why each half also asserts the accessor that is *supposed* to move.
    let mut devices = Devices::new();
    let mut input = snapshot();

    let mouse = table();
    devices.moved(Axis::MouseMotion, 50, 0);
    // A level on the same axis, so that a `snapshot` reading the wrong map
    // would have something to find rather than reading zero either way.
    devices.deflected(Axis::MouseMotion, 100, 0);
    devices.snapshot(&mouse, &mut input);
    assert_eq!(input.delta(action::LOOK).x, Signed16::from_bits(16_383));
    assert_eq!(
        input.analog(action::LOOK),
        Analog::ZERO,
        "an action bound to a mouse reads zero from the deflection accessor",
    );

    let stick = Bindings::new().axis(
        Axis::MouseMotion,
        action::LOOK,
        span(100),
        Reading::Deflection,
    );
    devices.moved(Axis::MouseMotion, 50, 0);
    devices.snapshot(&stick, &mut input);
    assert_eq!(input.analog(action::LOOK).x, Signed16::MAX);
    assert_eq!(
        input.delta(action::LOOK),
        Analog::ZERO,
        "an action bound to a stick reads zero from the displacement accessor",
    );
}

#[test]
fn a_deflection_survives_a_snapshot_and_focus_centres_it() {
    // The other half of "a level survives and a delta does not", for the kind
    // of reading nothing in this workspace produces. A stick that went back
    // to centre the frame after the platform last mentioned it would be
    // unplayable; one that stayed pushed while the player was in another window
    // would turn the camera on its own.
    let stick = Bindings::new().axis(
        Axis::MouseMotion,
        action::LOOK,
        span(100),
        Reading::Deflection,
    );
    let mut devices = Devices::new();
    let mut input = snapshot();

    devices.deflected(Axis::MouseMotion, 50, 0);
    devices.snapshot(&stick, &mut input);
    assert_eq!(input.analog(action::LOOK).x, Signed16::from_bits(16_383));

    devices.snapshot(&stick, &mut input);
    assert_eq!(input.analog(action::LOOK).x, Signed16::from_bits(16_383));

    devices.released_all();
    devices.snapshot(&stick, &mut input);
    assert_eq!(input.analog(action::LOOK), Analog::ZERO);
}

#[test]
fn motion_accumulates_between_snapshots() {
    // Four reports between two frames are one movement. A `moved` that replaced
    // rather than added would report the last event's delta, which on a mouse
    // sending one pixel at a time is a camera that barely turns however fast
    // the player moves.
    let mut devices = Devices::new();
    let mut input = snapshot();

    for _ in 0..4 {
        devices.moved(Axis::MouseMotion, 25, 0);
    }
    devices.snapshot(&table(), &mut input);

    assert_eq!(input.delta(action::LOOK).x, Signed16::MAX);
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
fn an_unbound_action_reads_released_however_hard_the_board_is_pressed() {
    // A table that bound nothing would still fill the snapshot if `snapshot`
    // wrote every action it could rather than every action it was told to.
    let nothing = Bindings::new();
    let mut devices = Devices::new();
    let mut input = snapshot();

    devices.press(Button::key(Key::Space));
    devices.press(Button::mouse(MouseButton::Left));
    devices.snapshot(&nothing, &mut input);

    assert_eq!(input.digital(action::NUDGE), Digital::RELEASED);
    assert_eq!(input.digital(action::FIRE), Digital::RELEASED);
}

#[test]
fn an_action_of_an_inactive_set_reads_released_while_its_key_is_down() {
    // The property the two halves of this crate have together and neither has
    // alone: the device layer records what the board is doing and the snapshot
    // decides who is allowed to hear it. Both assertions are needed — the same
    // key held with the owning set active is what says the binding works at
    // all, so the silence is silence rather than a table that binds nothing.
    let table = table().button(Button::key(Key::Enter), action::RESUME);
    let mut devices = Devices::new();
    let mut input = snapshot();

    devices.press(Button::key(Key::Enter));
    devices.snapshot(&table, &mut input);
    assert_eq!(input.digital(action::RESUME), Digital::RELEASED);

    input.activate(action::Paused::ID);
    devices.snapshot(&table, &mut input);
    assert_eq!(input.digital(action::RESUME), Digital::HELD);
    assert_eq!(input.digital(action::NUDGE), Digital::RELEASED);
}

#[test]
fn the_placeholder_binds_by_number_and_runs_out_rather_than_wrapping() {
    // The placeholder's whole contract is that it is arbitrary but total: every
    // digital action it reaches is bound to a different key, it binds them in
    // identifier order, and it stops when it runs out. A key bound to two
    // actions is the failure that would not show up as a missing binding.
    let table = Bindings::placeholder(action::SETS);

    let bound: Vec<DigitalId> = table.buttons().iter().map(|&(_, action)| action).collect();
    assert_eq!(
        bound,
        vec![DigitalId(0), DigitalId(1), DigitalId(2)],
        "three digital actions are declared, so three are bound, in order",
    );

    let mut keys: Vec<Button> = table.buttons().iter().map(|&(button, _)| button).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), 3, "two actions share a control");

    assert_eq!(
        table.axes().len(),
        1,
        "one analog action is declared, so the wheel is left unbound",
    );
    assert_eq!(table.axes()[0].action, AnalogId(0));

    // And that it stops. A declaration with more digital actions than there are
    // placeholder keys binds the keys it has and no more, rather than starting
    // over and giving one key two meanings.
    let crowded = Bindings::placeholder(many::SETS);
    let mut controls: Vec<Button> = crowded.buttons().iter().map(|&(b, _)| b).collect();
    let total = controls.len();
    assert!(total < 20, "twenty actions were all bound somehow");
    controls.sort_unstable();
    controls.dedup();
    assert_eq!(controls.len(), total, "a control was handed out twice");
}

#[test]
fn a_key_is_named_by_the_text_it_is_written_down_under() {
    // A binding file names keys, so the name and the variant have to survive
    // being written and read back. The round trip is checked over the whole
    // vocabulary rather than one key, and the names are checked for duplicates,
    // because two keys sharing a name is a file that reads back as the wrong
    // key rather than as an error.
    let mut names: Vec<&str> = Key::ALL.iter().map(|key| key.name()).collect();
    for key in Key::ALL {
        assert_eq!(Key::from_name(key.name()), Some(*key));
    }
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), total, "two keys are written down the same way");
    assert_eq!(Key::from_name("PauseBreak"), None);
}

// --- the raw control, which is the one thing a rebinding screen needs --------

#[test]
fn a_snapshot_reports_the_control_that_was_pressed_and_not_only_the_action() {
    // The route a rebinding screen asks its question through. Everything else
    // in a snapshot is an *action* — a game declares what it can be asked to do
    // and never sees a key code — and "press the control you want" is the one
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

// --- a pair of buttons standing in for a stick ------------------------------

#[test]
fn a_key_pair_reads_as_a_stick_pushed_either_way() {
    use corvid_input::platform::Component;

    // The composition, at the layer that owns it: a game reads
    // `Input::analog` and cannot tell whether the player used keys or a stick.
    let mut devices = Devices::new();
    let mut input = Input::new(action::SETS);
    let bindings = Bindings::new()
        .pair(
            Button::key(Key::S),
            Button::key(Key::W),
            action::LOOK,
            Component::Y,
        )
        .pair(
            Button::key(Key::A),
            Button::key(Key::D),
            action::LOOK,
            Component::X,
        );

    devices.press(Button::key(Key::W));
    devices.snapshot(&bindings, &mut input);
    assert_eq!(input.analog(action::LOOK).y, Signed16::MAX);
    assert_eq!(input.analog(action::LOOK).x, Signed16::ZERO);

    devices.press(Button::key(Key::A));
    devices.snapshot(&bindings, &mut input);
    assert_eq!(input.analog(action::LOOK).x, Signed16::MIN);

    // And it is a *level*, so it is still pushed on a frame in which the
    // platform said nothing at all. A displacement would have gone back to
    // zero here, which is the bug this distinction exists to prevent.
    devices.snapshot(&bindings, &mut input);
    assert_eq!(input.analog(action::LOOK).y, Signed16::MAX);

    devices.release(Button::key(Key::W));
    devices.snapshot(&bindings, &mut input);
    assert_eq!(input.analog(action::LOOK).y, Signed16::ZERO);
}

#[test]
fn both_halves_of_a_pair_held_is_exactly_centred() {
    use corvid_input::platform::Component;

    // Pressing left and right together stands still rather than creeping, and
    // *exactly* still: a composition that added the two contributions would
    // land a bit off centre and a player leaning on both would drift.
    let mut devices = Devices::new();
    let mut input = Input::new(action::SETS);
    let bindings = Bindings::new().pair(
        Button::key(Key::A),
        Button::key(Key::D),
        action::LOOK,
        Component::X,
    );

    devices.press(Button::key(Key::A));
    devices.press(Button::key(Key::D));
    devices.snapshot(&bindings, &mut input);
    assert_eq!(input.analog(action::LOOK).x, Signed16::ZERO);

    // Letting go of one leaves the other pushing, rather than leaving the axis
    // stuck at centre until both come up.
    devices.release(Button::key(Key::D));
    devices.snapshot(&bindings, &mut input);
    assert_eq!(input.analog(action::LOOK).x, Signed16::MIN);
}

#[test]
fn a_pair_answers_on_analog_and_leaves_delta_alone() {
    use corvid_input::platform::Component;

    // The half of `Reading` a pair cannot get wrong, because it has no field to
    // get it wrong with. A held button is a rate; a game that read this on
    // `delta` and skipped the `dt` would move by the frame time squared.
    let mut devices = Devices::new();
    let mut input = Input::new(action::SETS);
    let bindings = Bindings::new().pair(
        Button::key(Key::S),
        Button::key(Key::W),
        action::LOOK,
        Component::Y,
    );

    devices.press(Button::key(Key::W));
    devices.snapshot(&bindings, &mut input);
    assert_eq!(input.analog(action::LOOK).y, Signed16::MAX);
    assert_eq!(input.delta(action::LOOK), Analog::ZERO);
}

#[test]
fn a_stick_and_a_pair_on_one_action_follow_whichever_is_pushed_further() {
    use corvid_input::platform::{Component, Reading};

    // Two hands, one action. A player with keys and a pad plugged in should get
    // whichever they are actually using, which is the same union rule two
    // controls on one digital action already follow.
    let mut devices = Devices::new();
    let mut input = Input::new(action::SETS);
    let bindings = Bindings::new()
        .pair(
            Button::key(Key::S),
            Button::key(Key::W),
            action::LOOK,
            Component::Y,
        )
        .axis(
            Axis::LeftStick,
            action::LOOK,
            NonZeroU32::new(32_767).expect("not zero"),
            Reading::Deflection,
        );

    // The stick a little way over, and the key not pressed: the stick wins.
    devices.deflected(Axis::LeftStick, 0, 8_000);
    devices.snapshot(&bindings, &mut input);
    assert_eq!(input.analog(action::LOOK).y.to_bits(), 8_000);

    // Now the key as well, which is further: the key wins.
    devices.press(Button::key(Key::W));
    devices.snapshot(&bindings, &mut input);
    assert_eq!(input.analog(action::LOOK).y, Signed16::MAX);
}

#[test]
fn a_pad_button_is_a_button_like_any_other() {
    // The vocabulary, at the layer that folds it: nothing in `Devices` knows a
    // pad from a keyboard, which is what makes adding one a variant rather than
    // a code path.
    let mut devices = Devices::new();
    let mut input = Input::new(action::SETS);
    let bindings = Bindings::new()
        .button(Button::pad(PadButton::South), action::NUDGE)
        .button(Button::key(Key::Space), action::NUDGE);

    devices.press(Button::pad(PadButton::South));
    devices.snapshot(&bindings, &mut input);
    assert!(input.digital(action::NUDGE).pressed);

    // And it captures, so a rebinding screen can be answered with a pad.
    assert_eq!(input.captured(), Some(Button::pad(PadButton::South)));
}

// --- focus, which is the window and not an action ---------------------------

#[test]
fn focus_is_a_level_with_an_edge_either_side() {
    // A game that captures the pointer needs the *frame* the player came back
    // on, not merely that they are here now — so this is a `Digital` and the
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
    // platform stops reporting releases — and the *leaving itself* is reported,
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
