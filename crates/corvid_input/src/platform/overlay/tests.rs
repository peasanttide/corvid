//! What laying a player's table over the shipped one keeps, replaces, ignores
//! and holds back.

#![expect(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::num::NonZeroU32;

use crate::platform::bind::{Bindings, Component, Reading};
use crate::platform::table::{AxisEntry, ButtonEntry, PairEntry, Table};
use crate::source::{Axis, Button, Key, MouseButton};
use crate::{AnalogId, DigitalId, SetDescriptor, SetNames, layout};

/// Playing, with three buttons and a stick; and a menu beside it.
static SETS: [SetDescriptor; 2] = layout(&[
    SetNames {
        name: "Playing",
        digital: &["JUMP", "DUCK", "CROUCH"],
        analog: &["MOVE", "ZOOM"],
        pose: &[],
    },
    SetNames {
        name: "Menu",
        digital: &["BACK"],
        analog: &[],
        pose: &[],
    },
]);

fn shipped() -> Bindings {
    Bindings::new()
        .button(Button::key(Key::Space), DigitalId(0))
        .button(Button::key(Key::S), DigitalId(1))
        .button(Button::key(Key::C), DigitalId(2))
        .button(Button::key(Key::Tab), DigitalId(3))
        .pair(
            Button::key(Key::A),
            Button::key(Key::D),
            AnalogId(0),
            Component::X,
        )
        .axis(
            Axis::Scroll,
            AnalogId(1),
            NonZeroU32::new(1).unwrap(),
            Reading::Displacement,
        )
}

fn entry(control: &str, action: &str) -> ButtonEntry {
    ButtonEntry {
        control: control.to_string(),
        action: action.to_string(),
    }
}

fn player(buttons: Vec<ButtonEntry>) -> Table {
    Table {
        buttons,
        ..Table::default()
    }
}

fn played(table: &Table, shipped: &Bindings) -> Bindings {
    table
        .overlay(&SETS, shipped)
        .table
        .to_bindings(&SETS)
        .unwrap()
}

#[test]
fn an_empty_table_plays_what_the_game_ships() {
    let over = Table::default().overlay(&SETS, &shipped());
    assert_eq!(over.table.to_bindings(&SETS).unwrap(), shipped());
    assert!(over.ignored.is_empty() && over.withheld.is_empty());
}

#[test]
fn an_action_the_player_bound_takes_exactly_their_controls() {
    let bound = played(&player(vec![entry("W", "JUMP")]), &shipped());
    let jump: Vec<_> = bound
        .buttons()
        .iter()
        .filter(|(_, action)| *action == DigitalId(0))
        .collect();
    assert_eq!(jump, [&(Button::key(Key::W), DigitalId(0))]);
    assert!(
        bound
            .buttons()
            .contains(&(Button::key(Key::S), DigitalId(1)))
    );
}

#[test]
fn a_changed_default_reaches_a_player_who_never_touched_it() {
    // The whole reason defaults stay out of the player's table: a build that
    // gives DUCK another key gives it for everyone who left DUCK alone.
    let table = player(vec![entry("W", "JUMP")]);
    let changed = shipped().button(Button::key(Key::LeftControl), DigitalId(1));
    let bound = played(&table, &changed);
    assert!(
        bound
            .buttons()
            .contains(&(Button::key(Key::LeftControl), DigitalId(1)))
    );
}

#[test]
fn unbound_takes_every_control_off_an_action() {
    let table = Table {
        unbound: vec!["DUCK".to_string(), "MOVE".to_string()],
        ..Table::default()
    };
    let bound = played(&table, &shipped());
    assert!(bound.buttons().iter().all(|(_, a)| *a != DigitalId(1)));
    assert!(bound.pairs().is_empty());
}

#[test]
fn a_name_this_build_does_not_declare_is_ignored_and_reported() {
    // Dropped or mistyped, the table cannot say which, so neither stops the
    // run; a button on an analog action is the wrong kind and goes the same way.
    let table = Table {
        buttons: vec![entry("R", "ROLL"), entry("Q", "MOVE")],
        unbound: vec!["SPRINT".to_string()],
        ..Table::default()
    };
    let over = table.overlay(&SETS, &shipped());
    assert_eq!(over.ignored, ["MOVE", "ROLL", "SPRINT"]);
    assert_eq!(over.table.to_bindings(&SETS).unwrap(), shipped());
}

#[test]
fn a_control_the_player_moved_is_withheld_from_its_old_action() {
    let over = player(vec![entry("C", "JUMP")]).overlay(&SETS, &shipped());
    assert_eq!(over.withheld, ["C"]);
    let bound = over.table.to_bindings(&SETS).unwrap();
    assert!(bound.buttons().iter().all(|(_, a)| *a != DigitalId(2)));
}

#[test]
fn a_control_used_in_another_set_is_not_withheld() {
    // Tab is BACK in the menu; Playing and Menu never answer together.
    let over = player(vec![entry("Tab", "JUMP")]).overlay(&SETS, &shipped());
    assert!(over.withheld.is_empty(), "{:?}", over.withheld);
}

#[test]
fn pairs_and_axes_take_controls_too() {
    // The player's pair on C/V takes C from CROUCH, and their Scroll on MOVE
    // takes it from ZOOM.
    let table = Table {
        pairs: vec![PairEntry {
            low: "C".to_string(),
            high: "V".to_string(),
            action: "MOVE".to_string(),
            component: Component::Y,
        }],
        axes: vec![AxisEntry {
            control: "Scroll".to_string(),
            action: "MOVE".to_string(),
            span: 1,
            reading: Reading::Displacement,
        }],
        ..Table::default()
    };
    let over = table.overlay(&SETS, &shipped());
    assert_eq!(over.withheld, ["C", "Scroll"]);
    let bound = over.table.to_bindings(&SETS).unwrap();
    assert!(bound.axes().iter().all(|b| b.action != AnalogId(1)));

    // MOVE is the player's, so the shipped A/D pair is not.
    let bound_pairs: Vec<_> = bound.pairs().iter().map(|p| p.low).collect();
    assert_eq!(bound_pairs, [Button::key(Key::C)]);
}

#[test]
fn a_control_is_compared_by_what_it_denotes() {
    let shipped = Bindings::new().button(Button::Mouse(MouseButton::Other(4)), DigitalId(2));
    let over = player(vec![entry("Mouse04", "JUMP")]).overlay(&SETS, &shipped);
    assert_eq!(over.withheld, ["Mouse4"]);
}

#[test]
fn a_control_this_build_cannot_name_is_left_for_to_bindings_to_refuse() {
    let over = player(vec![entry("Foot", "JUMP")]).overlay(&SETS, &shipped());
    assert!(over.table.to_bindings(&SETS).is_err());
}
