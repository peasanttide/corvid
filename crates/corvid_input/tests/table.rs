//! A binding table written down by name, which is what a file holds.
//!
//! The property under all of this is that a file names an action by the
//! identifier a programmer *declared* it under and never by the number the
//! declaration handed it. Numbers come from declaration order -- the crate's own
//! documentation calls that a wire format -- so a file recording `4` would point
//! at somebody else's action the next time an action was inserted above it,
//! silently, with nothing to compare against.

#![cfg(all(feature = "platform", feature = "serde"))]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

use core::num::NonZeroU32;

use corvid_input::platform::{Axis, Bindings, Button, Key, MouseButton, Reading, Table, Unknown};
use corvid_input::{AnalogId, DigitalId, analog_named, digital_name, digital_named};

mod action {
    corvid_input::action_sets! {
        pub set Menu {
            digital CONFIRM, BACK;
        }
        pub set Playing {
            digital FORWARD, FIRE;
            analog LOOK;
        }
    }
}

/// A span, without a panic in sight.
const fn span(units: u32) -> NonZeroU32 {
    NonZeroU32::new(units).expect("the spans in this file are not zero")
}

/// A table with both shapes a map could not hold in it.
fn table() -> Bindings {
    Bindings::new()
        // Two controls on one action, which is how a game is playable with
        // either hand.
        .button(Button::key(Key::W), action::FORWARD)
        .button(Button::key(Key::ArrowUp), action::FORWARD)
        // And one control on two actions, which is how a chord is expressed
        // without this table learning what the word means.
        .button(Button::mouse(MouseButton::Left), action::FIRE)
        .button(Button::mouse(MouseButton::Left), action::CONFIRM)
        .axis(
            Axis::MouseMotion,
            action::LOOK,
            span(320),
            Reading::Displacement,
        )
}

#[test]
fn a_table_survives_a_round_trip_with_its_order_intact() {
    let written = Table::from_bindings(&table(), action::SETS);
    let read = written
        .to_bindings(action::SETS)
        .expect("it names what it was written from");
    assert_eq!(read, table());
    // Order, explicitly: the accessors promise "in the order it was added", and
    // a round trip that sorted would be a round trip that changed which control
    // a rebinding screen shows first.
    assert_eq!(read.buttons(), table().buttons());
}

#[test]
fn what_goes_in_the_file_is_the_name_and_never_the_number() {
    let written = Table::from_bindings(&table(), action::SETS);
    let json = serde_json::to_string(&written).expect("a table serializes");
    assert!(json.contains("FORWARD"), "{json}");
    assert!(json.contains("MouseLeft"), "{json}");
    assert!(json.contains("MouseMotion"), "{json}");
    // The numbers are what would rot. `FIRE` is `DigitalId(3)` today and would
    // be `DigitalId(4)` the moment somebody added an action to `Menu`.
    assert_eq!(action::FIRE, DigitalId(3));
    assert!(!json.contains(r#""action":3"#), "{json}");
}

#[test]
fn the_json_a_player_edits_is_the_json_this_reads() {
    // Written by hand, as somebody with a text editor would: this is the shape
    // documented in the design and it has to parse.
    let hand_written = r#"{
      "buttons": [
        { "control": "W", "action": "FORWARD" },
        { "control": "Up", "action": "FORWARD" },
        { "control": "MouseLeft", "action": "FIRE" }
      ],
      "axes": [
        { "control": "MouseMotion", "action": "LOOK",
          "span": 640, "reading": "Displacement" }
      ]
    }"#;
    let parsed: Table = serde_json::from_str(hand_written).expect("it is a table");
    let bindings = parsed
        .to_bindings(action::SETS)
        .expect("it names what this build has");

    assert_eq!(bindings.buttons().len(), 3);
    assert_eq!(bindings.axes()[0].span, span(640));
    assert_eq!(bindings.axes()[0].action, action::LOOK);
}

#[test]
fn either_list_may_be_missing_altogether() {
    // A player who deleted the axes to get the mouse back to its default should
    // get a file that still loads rather than a run that refuses.
    let parsed: Table = serde_json::from_str(r#"{ "buttons": [] }"#).expect("it is a table");
    assert!(
        parsed
            .to_bindings(action::SETS)
            .expect("it loads")
            .axes()
            .is_empty()
    );
    let empty: Table = serde_json::from_str("{}").expect("an empty table is a table");
    assert_eq!(
        empty.to_bindings(action::SETS).expect("it loads"),
        Bindings::new()
    );
}

#[test]
fn a_name_this_build_does_not_have_is_refused_and_named() {
    let bad = |json: &str| -> Unknown {
        serde_json::from_str::<Table>(json)
            .expect("it is a table")
            .to_bindings(action::SETS)
            .expect_err("it names something that is not here")
    };

    assert_eq!(
        bad(r#"{ "buttons": [{ "control": "W", "action": "FOWARD" }] }"#),
        Unknown::DigitalAction("FOWARD".into()),
    );
    assert_eq!(
        bad(r#"{ "buttons": [{ "control": "Foot", "action": "FORWARD" }] }"#),
        Unknown::Control("Foot".into()),
    );
    assert_eq!(
        bad(r#"{ "axes": [{ "control": "Treadmill", "action": "LOOK",
                            "span": 1, "reading": "Displacement" }] }"#),
        Unknown::Axis("Treadmill".into()),
    );
    assert_eq!(
        bad(r#"{ "axes": [{ "control": "MouseMotion", "action": "LOOK",
                            "span": 0, "reading": "Displacement" }] }"#),
        Unknown::Span("LOOK".into()),
    );

    // An analog action named where a digital one belongs is refused too, rather
    // than matching across the two identifier spaces -- they are numbered apart
    // and `LOOK` is not a button.
    assert_eq!(
        bad(r#"{ "buttons": [{ "control": "W", "action": "LOOK" }] }"#),
        Unknown::DigitalAction("LOOK".into()),
    );
}

#[test]
fn the_message_a_player_reads_names_what_they_typed() {
    let why = Unknown::DigitalAction("FOWARD".into()).to_string();
    assert!(why.contains("FOWARD"), "{why}");
}

#[test]
fn a_binding_whose_action_has_no_name_is_left_out_rather_than_refused() {
    // The asymmetry between the two directions. Writing a file is something
    // done *for* a player, and a table bound by hand outside the declaration
    // has bindings there is nothing to call -- so those are dropped. Reading is
    // where a name somebody typed did not match, and that refuses.
    let outside = Bindings::new()
        .button(Button::key(Key::W), action::FORWARD)
        .button(Button::key(Key::Z), DigitalId(9_000));
    let written = Table::from_bindings(&outside, action::SETS);
    assert_eq!(written.buttons.len(), 1);
    assert_eq!(written.buttons[0].action, "FORWARD");
}

#[test]
fn the_names_are_the_identifiers_the_declaration_was_written_with() {
    // What the macro generates, stated plainly: the name is the constant's own
    // spelling, which is the one thing about an action that does not move.
    assert_eq!(digital_name(action::SETS, action::FORWARD), Some("FORWARD"));
    assert_eq!(
        digital_named(action::SETS, "CONFIRM"),
        Some(action::CONFIRM)
    );
    assert_eq!(analog_named(action::SETS, "LOOK"), Some(action::LOOK));

    // The two identifier spaces stay apart: `DigitalId(0)` and `AnalogId(0)`
    // are different actions, so a lookup of one kind never answers the other's.
    assert_eq!(action::CONFIRM, DigitalId(0));
    assert_eq!(action::LOOK, AnalogId(0));
    assert_eq!(digital_named(action::SETS, "LOOK"), None);
    assert_eq!(analog_named(action::SETS, "CONFIRM"), None);

    // And a name nothing declared is `None` rather than a guess.
    assert_eq!(digital_named(action::SETS, "forward"), None);
    assert_eq!(digital_name(action::SETS, DigitalId(9_000)), None);
}

#[test]
fn every_control_this_vocabulary_names_reads_back_as_itself() {
    // The round trip at the level below the table: a file writes a control with
    // `Display` and reads it with `from_name`, so a control the two disagree
    // about is a binding that vanishes when it is saved.
    for key in Key::ALL {
        let written = Button::key(*key).to_string();
        assert_eq!(
            Button::from_name(&written),
            Some(Button::key(*key)),
            "{written} did not read back",
        );
    }
    for button in [
        MouseButton::Left,
        MouseButton::Right,
        MouseButton::Middle,
        MouseButton::Other(4),
        MouseButton::Other(u16::MAX),
    ] {
        let written = Button::mouse(button).to_string();
        assert_eq!(Button::from_name(&written), Some(Button::mouse(button)));
    }
    for axis in Axis::ALL {
        assert_eq!(Axis::from_name(axis.name()), Some(*axis));
    }

    // And a word that is nearly a control is not one.
    assert_eq!(Button::from_name("Mouse"), None);
    assert_eq!(Button::from_name("MouseLeftish"), None);
    assert_eq!(Button::from_name(""), None);
}
