//! What `action_sets!` hands out, and what the table it leaves behind says.

use corvid_input::{AnalogId, DigitalId, PoseId, SetId};

/// The declaration the rest of this file is about: three sets, every kind of
/// action, and one set that declares only digitals so that an empty range is
/// covered as well as a full one.
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

/// The same three sets, declared a second time in a second module.
///
/// Word for word: two invocations that read alike have to number alike, or a
/// game that splits its declaration across a refactor gets two binding files
/// that disagree about which action is which.
pub mod twin {
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

/// The same three sets with the first two swapped, which is the edit the
/// documentation calls a wire break.
pub mod swapped {
    corvid_input::action_sets! {
        pub set Build {
            digital PLACE, CANCEL, ROTATE_CW, ROTATE_CCW;
            analog LOOK, MOVE;
            pose POINTER;
        }
        pub set Menu {
            digital NAVIGATE_UP, NAVIGATE_DOWN, ACTIVATE, BACK;
        }
        pub set Swarm {
            digital DROP, ABILITY_EMP, ABILITY_BURROW;
            analog SPIN, ZOOM;
            pose GRAB_LEFT, GRAB_RIGHT;
        }
    }
}

/// A declaration that names one action of each kind and nothing else, for the
/// edges the big one cannot reach.
pub mod solo {
    corvid_input::action_sets! {
        pub set Only {
            digital PUSH;
            analog SLIDE;
            pose HAND;
        }
    }
}

#[test]
fn identifiers_are_dense_and_in_declaration_order() {
    assert_eq!(action::Menu::ID, SetId(0));
    assert_eq!(action::Build::ID, SetId(1));
    assert_eq!(action::Swarm::ID, SetId(2));

    // Digital actions are one space, numbered straight through the sets.
    assert_eq!(action::NAVIGATE_UP, DigitalId(0));
    assert_eq!(action::NAVIGATE_DOWN, DigitalId(1));
    assert_eq!(action::ACTIVATE, DigitalId(2));
    assert_eq!(action::BACK, DigitalId(3));
    assert_eq!(action::PLACE, DigitalId(4));
    assert_eq!(action::ROTATE_CCW, DigitalId(7));
    assert_eq!(action::DROP, DigitalId(8));
    assert_eq!(action::ABILITY_BURROW, DigitalId(10));

    // Analog actions are another, and it starts over at zero -- `Menu` declared
    // none, so `Build`'s first analog action is `AnalogId(0)` even though its
    // first digital action is `DigitalId(4)`.
    assert_eq!(action::LOOK, AnalogId(0));
    assert_eq!(action::MOVE, AnalogId(1));
    assert_eq!(action::SPIN, AnalogId(2));
    assert_eq!(action::ZOOM, AnalogId(3));

    // And poses are a third.
    assert_eq!(action::POINTER, PoseId(0));
    assert_eq!(action::GRAB_LEFT, PoseId(1));
    assert_eq!(action::GRAB_RIGHT, PoseId(2));
}

#[test]
fn two_invocations_of_the_same_declaration_agree() {
    assert_eq!(action::SETS.len(), twin::SETS.len());
    for (here, there) in action::SETS.iter().zip(twin::SETS) {
        assert_eq!(here, there, "{} was numbered twice over", here.name());
    }

    assert_eq!(action::Menu::ID, twin::Menu::ID);
    assert_eq!(action::Swarm::ID, twin::Swarm::ID);
    assert_eq!(action::NAVIGATE_UP, twin::NAVIGATE_UP);
    assert_eq!(action::ABILITY_BURROW, twin::ABILITY_BURROW);
    assert_eq!(action::LOOK, twin::LOOK);
    assert_eq!(action::GRAB_RIGHT, twin::GRAB_RIGHT);
}

#[test]
fn reordering_a_declaration_moves_the_identifiers() {
    // The point of this test is not that the numbers are wrong when the
    // declaration moves -- they are exactly as right as before, for a different
    // declaration. It is that they are *different*, which is what makes a saved
    // binding file worthless and what the documentation warns about.
    assert_eq!(swapped::Build::ID, SetId(0));
    assert_eq!(swapped::Menu::ID, SetId(1));

    assert_ne!(action::PLACE, swapped::PLACE);
    assert_ne!(action::NAVIGATE_UP, swapped::NAVIGATE_UP);
    assert_eq!(swapped::PLACE, DigitalId(0));
    assert_eq!(swapped::NAVIGATE_UP, DigitalId(4));

    // The third set did not move, and neither did anything numbered in a space
    // the swap did not disturb: `Swarm`'s digitals still start after eight
    // because the two sets ahead of it still declare eight between them, and
    // `Swarm`'s analogs still start after two.
    assert_eq!(action::DROP, swapped::DROP);
    assert_eq!(action::SPIN, swapped::SPIN);
    assert_eq!(action::GRAB_LEFT, swapped::GRAB_LEFT);

    // Which is worth being exact about: reordering does not move *everything*,
    // so a game that checks one identifier and finds it unmoved has learnt
    // nothing. `LOOK` and `MOVE` belong to `Build` and stayed at zero and one
    // through a swap that moved every digital action in both sets.
    assert_eq!(action::LOOK, swapped::LOOK);
}

#[test]
fn the_table_partitions_each_kind_of_identifier() {
    let mut id = 0u16;
    let mut digital = 0;
    let mut analog = 0;
    let mut pose = 0;

    for set in action::SETS {
        // A set's number is its position, so the table can be indexed by it.
        assert_eq!(set.id(), SetId(id), "set {id} is numbered {}", set.id());

        // No gap and no overlap: each set's run starts where the last one
        // ended. A numbering that restarted per set, or that used one counter
        // for all three kinds, fails here.
        assert_eq!(set.digital().first(), digital);
        assert_eq!(set.analog().first(), analog);
        assert_eq!(set.pose().first(), pose);

        digital += set.digital().count();
        analog += set.analog().count();
        pose += set.pose().count();
        id += 1;
    }

    assert_eq!((id, digital, analog, pose), (3, 11, 4, 3));
}

#[test]
fn a_set_owns_exactly_its_own_identifiers() {
    let menu = action::SETS[0];
    let build = action::SETS[1];

    assert!(menu.digital().contains(action::BACK.0));
    assert!(build.digital().contains(action::PLACE.0));

    // The boundary in both directions. `BACK` is the last of `Menu` and `PLACE`
    // is the first of `Build`, so an off-by-one at either end of the range
    // check shows up here and nowhere else in this file.
    assert!(!menu.digital().contains(action::PLACE.0));
    assert!(!build.digital().contains(action::BACK.0));

    // And an identifier past the end of every set.
    assert!(!build.digital().contains(11));
}

#[test]
fn a_set_that_declares_nothing_of_a_kind_owns_nothing_of_it() {
    let menu = action::SETS[0];

    assert!(menu.analog().is_empty());
    assert!(menu.pose().is_empty());

    // Including the identifier the empty range's `first` happens to report,
    // which is the running total at the point the set was declared and is not
    // a member of anything.
    assert!(!menu.analog().contains(menu.analog().first()));
    assert!(!menu.pose().contains(menu.pose().first()));
}

#[test]
fn the_set_marker_carries_its_number_and_name() {
    assert_eq!(action::Build::NAME, "Build");
    assert_eq!(
        action::SETS[usize::from(action::Build::ID.0)].name(),
        "Build"
    );
    assert_eq!(solo::Only::NAME, "Only");
}

#[test]
fn a_lone_set_numbers_every_kind_from_zero() {
    assert_eq!(solo::Only::ID, SetId(0));
    assert_eq!(solo::PUSH, DigitalId(0));
    assert_eq!(solo::SLIDE, AnalogId(0));
    assert_eq!(solo::HAND, PoseId(0));
    assert_eq!(solo::SETS.len(), 1);
}
