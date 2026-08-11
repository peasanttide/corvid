//! The frozen numbering. **Changing a value in this file is a wire-format
//! break.**
//!
//! `action_sets!` hands out identifiers from declaration order, a binding file
//! records that this player's `X` button is `DigitalId(4)`, and that file
//! outlives the build that wrote it. Move a declaration and the file now points
//! at somebody else's action -- with no compile error, no failed parse, and
//! nothing to notice it but a player whose controller stopped doing what they
//! set it to. So the numbering for a fixed declaration is written down here as
//! literals, over a declaration shaped like a game's rather than a fixture
//! invented for the test, so that the table below is the same shape a real
//! binding file is read against.
//!
//! Nothing in this crate is an enum, so there are no discriminants of its own
//! to freeze -- the identifiers *are* the discriminants, and they are this
//! table.
//!
//! This is not a test to regenerate. If a change is genuinely wanted it is a
//! new version of the format: bump the crate's major version, and give bindings
//! saved under the old numbering somewhere to go. Making a red row green by
//! pasting the new value in is how a player loses their bindings silently.
//!
//! The *encoding* those identifiers travel in is frozen separately, in
//! `tests/encoding.rs`, split off because a file stays under 400 lines.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

// Only the encoding tables name these types. The numbering tests read the
// identifiers through the constants the macro generated and never have to say
// what they are, so without `serde` this import is unused and warns.

/// The declaration every table below is frozen against: a menu, a build mode
/// and a swarm mode, with all three kinds of action between them.
pub mod action {
    corvid_input::action_sets! {
        pub set Menu {
            digital NAVIGATE_UP, NAVIGATE_DOWN, ACTIVATE, BACK;
        }
        pub set Build {
            digital PLACE, CANCEL, ROTATE_CW, ROTATE_CCW;
            analog LOOK, MOVE;
            pose POINTER;
        }
        pub set Swarm {
            digital DROP, ABILITY_EMP, ABILITY_BURROW;
            analog SPIN, ZOOM;
            pose GRAB_LEFT, GRAB_RIGHT;
        }
    }
}

/// One row of the set table: name, number, and the first and count of each of
/// the three kinds.
type SetRow = (&'static str, u16, u16, u16, u16, u16, u16, u16);

/// Every set, as `(name, id, first digital, digitals, first analog, analogs,
/// first pose, poses)`.
///
/// The three `first` columns are what a swap of two declarations moves, and
/// they are three columns rather than one because the three kinds are numbered
/// in separate spaces: an implementation that ran one counter across all three
/// would put `Build`'s analog actions at four, and only this table would say
/// so.
const GOLDEN_SETS: &[SetRow] = &[
    ("Menu", 0, 0, 4, 0, 0, 0, 0),
    ("Build", 1, 4, 4, 0, 2, 0, 1),
    ("Swarm", 2, 8, 3, 2, 2, 1, 2),
];

/// Every digital action and the number it was given.
const GOLDEN_DIGITAL: &[(&str, u16)] = &[
    ("NAVIGATE_UP", 0),
    ("NAVIGATE_DOWN", 1),
    ("ACTIVATE", 2),
    ("BACK", 3),
    ("PLACE", 4),
    ("CANCEL", 5),
    ("ROTATE_CW", 6),
    ("ROTATE_CCW", 7),
    ("DROP", 8),
    ("ABILITY_EMP", 9),
    ("ABILITY_BURROW", 10),
];

/// Every analog action. `Menu` declares none, so `Build`'s start at zero while
/// its digital actions start at four -- the row that pins the spaces apart.
const GOLDEN_ANALOG: &[(&str, u16)] = &[("LOOK", 0), ("MOVE", 1), ("SPIN", 2), ("ZOOM", 3)];

/// Every pose.
const GOLDEN_POSE: &[(&str, u16)] = &[("POINTER", 0), ("GRAB_LEFT", 1), ("GRAB_RIGHT", 2)];

#[test]
fn the_table_is_what_it_was_recorded_as() {
    let found: Vec<SetRow> = action::SETS
        .iter()
        .map(|set| {
            (
                set.name(),
                set.id().0,
                set.digital().first(),
                set.digital().count(),
                set.analog().first(),
                set.analog().count(),
                set.pose().first(),
                set.pose().count(),
            )
        })
        .collect();

    assert_eq!(
        found, GOLDEN_SETS,
        "the numbering moved, which is a wire-format break and not a test to \
         regenerate: every binding file saved under the recorded numbering now \
         names a different action",
    );
}

#[test]
fn every_digital_action_is_numbered_as_it_was_recorded() {
    let found = [
        ("NAVIGATE_UP", action::NAVIGATE_UP),
        ("NAVIGATE_DOWN", action::NAVIGATE_DOWN),
        ("ACTIVATE", action::ACTIVATE),
        ("BACK", action::BACK),
        ("PLACE", action::PLACE),
        ("CANCEL", action::CANCEL),
        ("ROTATE_CW", action::ROTATE_CW),
        ("ROTATE_CCW", action::ROTATE_CCW),
        ("DROP", action::DROP),
        ("ABILITY_EMP", action::ABILITY_EMP),
        ("ABILITY_BURROW", action::ABILITY_BURROW),
    ];
    check(GOLDEN_DIGITAL, &found.map(|(name, id)| (name, id.0)));
}

#[test]
fn every_analog_action_is_numbered_as_it_was_recorded() {
    let found = [
        ("LOOK", action::LOOK),
        ("MOVE", action::MOVE),
        ("SPIN", action::SPIN),
        ("ZOOM", action::ZOOM),
    ];
    check(GOLDEN_ANALOG, &found.map(|(name, id)| (name, id.0)));
}

#[test]
fn every_pose_is_numbered_as_it_was_recorded() {
    let found = [
        ("POINTER", action::POINTER),
        ("GRAB_LEFT", action::GRAB_LEFT),
        ("GRAB_RIGHT", action::GRAB_RIGHT),
    ];
    check(GOLDEN_POSE, &found.map(|(name, id)| (name, id.0)));
}

/// Compares a labelled fixture against its recorded row and reports every row
/// that moved at once.
///
/// One row at a time would be the obvious way to write this and the wrong one.
/// A reordered declaration moves a run of rows, and the count and the run's
/// shape are the first two things worth knowing, so both are in the message and
/// the message is formatted the way the table is written.
fn check(table: &[(&str, u16)], found: &[(&str, u16)]) {
    assert_eq!(
        table.len(),
        found.len(),
        "the table has {} rows and the declaration has {}",
        table.len(),
        found.len(),
    );

    let moved: Vec<String> = table
        .iter()
        .zip(found)
        .filter(|((_, recorded), (_, actual))| recorded != actual)
        .map(|((label, _), (_, actual))| format!("    ({label:?}, {actual}),"))
        .collect();

    assert!(
        moved.is_empty(),
        "{} of {} recorded identifiers moved, which is a wire-format break and \
         not a test to regenerate:\n{}",
        moved.len(),
        table.len(),
        moved.join("\n"),
    );
}
