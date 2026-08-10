//! What a binding table is: which controls reach which action, and what an
//! action reads when nothing does.
//!
//! Split from the other two platform suites because a file stays under 400
//! lines, and this is the seam that was already there: every question here is
//! about the *table* and is answered by a single reading, where the other two
//! ask about an interval between two of them.

#![cfg(feature = "platform")]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{HALF_SPAN, action, many, snapshot, span, table};

use corvid_fixed::Signed16;
use corvid_input::platform::{
    Axis, Bindings, Button, Devices, Key, MouseButton, PadButton, Reading,
};
use corvid_input::{Analog, AnalogId, Digital, DigitalId, Input};

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
    // own reading over whatever the others said -- and a control nobody is
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
        HALF_SPAN,
        "the quiet scroll binding zeroed what the mouse reported",
    );

    // And the other way round, so the answer does not depend on which row is
    // last.
    devices.moved(Axis::Scroll, 50, 0);
    devices.snapshot(&both, &mut input);
    assert_eq!(
        input.delta(action::LOOK).x,
        HALF_SPAN,
        "the quiet mouse binding zeroed what the scroll reported",
    );

    // Neither moving is still zero, which is what says the union is a union
    // rather than a value that sticks.
    devices.snapshot(&both, &mut input);
    assert_eq!(input.delta(action::LOOK), Analog::ZERO);
}

#[test]
fn a_displacement_binding_answers_on_delta_and_a_deflection_binding_on_analog() {
    // The split, as the one property that makes reaching for the wrong
    // accessor a value that stays still. Both directions are asserted, because
    // a `snapshot` that wrote both accessors from one reading would satisfy
    // either half on its own -- and so would one that wrote neither, which is
    // why each half also asserts the accessor that is *supposed* to move.
    let mut devices = Devices::new();
    let mut input = snapshot();

    let mouse = table();
    devices.moved(Axis::MouseMotion, 50, 0);
    // A level on the same axis, so that a `snapshot` reading the wrong map
    // would have something to find rather than reading zero either way.
    devices.deflected(Axis::MouseMotion, 100, 0);
    devices.snapshot(&mouse, &mut input);
    assert_eq!(input.delta(action::LOOK).x, HALF_SPAN);
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
    // decides who is allowed to hear it. Both assertions are needed -- the same
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
    // digital action it reaches is bound to a key *and* to the pad button
    // standing in for it, it binds them in identifier order, and it stops when
    // it runs out. A control bound to two actions is the failure that would not
    // show up as a missing binding.
    let table = Bindings::placeholder(action::SETS);

    let bound: Vec<DigitalId> = table.buttons().iter().map(|&(_, action)| action).collect();
    assert_eq!(
        bound,
        vec![
            DigitalId(0),
            DigitalId(0),
            DigitalId(1),
            DigitalId(1),
            DigitalId(2),
            DigitalId(2),
        ],
        "three digital actions are declared, so three are bound, each on both kinds of hardware",
    );

    // Each action reaches a board and a pad, which is the property that makes
    // the table worth something to a player holding either.
    for action in [DigitalId(0), DigitalId(1), DigitalId(2)] {
        let mut kinds: Vec<&str> = table
            .buttons()
            .iter()
            .filter(|&&(_, bound)| bound == action)
            .map(|&(button, _)| match button {
                Button::Key(_) => "key",
                Button::Pad(_) => "pad",
                _ => "some other kind of control",
            })
            .collect();
        kinds.sort_unstable();
        assert_eq!(kinds, vec!["key", "pad"], "{action:?} is not on both");
    }

    let mut controls: Vec<Button> = table.buttons().iter().map(|&(button, _)| button).collect();
    controls.sort_unstable();
    controls.dedup();
    assert_eq!(controls.len(), 6, "two actions share a control");

    // Neither the arrows nor the d-pad are handed out, because both are how a
    // player expects to move rather than to act.
    for button in table.buttons() {
        assert!(
            !matches!(
                button.0,
                Button::Key(Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight)
                    | Button::Pad(
                        PadButton::PadUp
                            | PadButton::PadDown
                            | PadButton::PadLeft
                            | PadButton::PadRight
                    )
            ),
            "{:?} is a direction and should not be a placeholder",
            button.0,
        );
        assert_ne!(
            button.0,
            Button::pad(PadButton::Guide),
            "the system button belongs to the platform",
        );
    }

    // One analog action is declared, so it takes the mouse and the stick and
    // the wheel is left unbound.
    let axes: Vec<Axis> = table.axes().iter().map(|binding| binding.axis).collect();
    assert_eq!(axes, vec![Axis::MouseMotion, Axis::RightStick]);
    assert!(table.axes().iter().all(|b| b.action == AnalogId(0)));

    // And that it stops. A declaration with more digital actions than there are
    // placeholder controls binds the ones it has and no more, rather than
    // starting over and giving one control two meanings.
    let crowded = Bindings::placeholder(many::SETS);
    let mut controls: Vec<Button> = crowded.buttons().iter().map(|&(b, _)| b).collect();
    let total = controls.len();
    assert!(total < 40, "twenty actions were all bound somehow");
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
