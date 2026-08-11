//! The controls that carry a quantity: sticks, mice, and the key pairs that
//! stand in for a stick.
//!
//! Split from the other two platform suites because a file stays under 400
//! lines, and this is the seam that was already there: an analog control has a
//! *scale* as well as a state, so every test here is about how far something
//! moved rather than about whether it did.

#![cfg(feature = "platform")]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{HALF_SPAN, action, snapshot, span, table};

use core::num::NonZeroU32;

use corvid_fixed::Signed16;
use corvid_input::platform::{Axis, Bindings, Button, Devices, Key, Reading};
use corvid_input::{Analog, Input};

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
    assert_eq!(input.analog(action::LOOK).x, HALF_SPAN);

    devices.snapshot(&stick, &mut input);
    assert_eq!(input.analog(action::LOOK).x, HALF_SPAN);

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
    assert_eq!(input.delta(action::LOOK).x, HALF_SPAN);

    devices.moved(Axis::MouseMotion, -100_000, 0);
    devices.snapshot(&table, &mut input);
    assert_eq!(input.delta(action::LOOK).x, Signed16::MIN);
}
