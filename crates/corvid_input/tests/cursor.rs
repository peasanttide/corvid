//! What a pointer mode means, and what happens when the platform says no.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_input::{Cursor, Input, SetDescriptor, action_sets};

action_sets! {
    pub set Playing {
        digital FIRE;
        analog LOOK;
    }
}

/// The two axes the four modes are the corners of.
#[test]
fn the_four_modes_are_two_decisions() {
    assert!(Cursor::Free.is_visible() && !Cursor::Free.is_grabbed());
    assert!(!Cursor::Hidden.is_visible() && !Cursor::Hidden.is_grabbed());
    assert!(Cursor::Confined.is_visible() && Cursor::Confined.is_grabbed());
    assert!(!Cursor::Locked.is_visible() && Cursor::Locked.is_grabbed());
}

/// Only one of them pins the pointer, and it is the one a camera asks about.
#[test]
fn only_one_mode_is_locked() {
    assert!(Cursor::Locked.is_locked());
    for other in [Cursor::Free, Cursor::Hidden, Cursor::Confined] {
        assert!(!other.is_locked(), "{other:?}");
    }
}

/// The fallback chain ends at `Free`, which no platform refuses.
///
/// This is what makes a refused request degrade rather than fail: a game asking
/// for a lock it cannot have still gets the strongest thing the platform will
/// give it.
#[test]
fn every_fallback_chain_terminates() {
    for start in [
        Cursor::Free,
        Cursor::Hidden,
        Cursor::Confined,
        Cursor::Locked,
    ] {
        let mut mode = start;
        let mut steps = 0;
        while let Some(next) = mode.fallback() {
            mode = next;
            steps += 1;
            assert!(steps < 8, "{start:?} does not terminate");
        }
        assert_eq!(mode, Cursor::Free, "{start:?} ended somewhere else");
    }
}

/// A lock degrades to a confinement rather than straight to nothing, which is
/// the whole reason the chain has three links rather than two.
#[test]
fn a_refused_lock_is_still_a_grab() {
    let next = Cursor::Locked
        .fallback()
        .expect("a lock has somewhere to go");
    assert!(next.is_grabbed());
    assert_eq!(next, Cursor::Confined);
}

/// A snapshot starts free, because that is what a window does when nobody has
/// said otherwise — and it is the mode a player can always get out of.
#[test]
fn a_new_snapshot_is_free() {
    assert_eq!(Input::new(SETS).cursor(), Cursor::Free);
}

/// The mode is a level rather than an edge: settling a snapshot spends the
/// presses and the displacements and leaves it alone, because the pointer is
/// still in whatever mode it is in.
#[test]
fn settling_does_not_release_the_pointer() {
    let mut input = Input::new(SETS);
    input.set_cursor(Cursor::Locked);
    input.settle();
    assert_eq!(input.cursor(), Cursor::Locked);
}

/// Absorbing takes the freshest reading, for the same reason.
#[test]
fn absorbing_takes_the_freshest_mode() {
    let mut input = Input::new(SETS);
    input.set_cursor(Cursor::Locked);

    let mut fresh = Input::new(SETS);
    fresh.set_cursor(Cursor::Free);

    input.absorb(&fresh);
    assert_eq!(input.cursor(), Cursor::Free);
}

/// Clearing keeps the pointer's mode, because clearing is about *device
/// readings* and a pointer mode is not one.
///
/// # This test used to assert the opposite, and that was the bug
///
/// It said clearing went "back to what a window does when nobody has said
/// otherwise", which sounds right and made `Input::cursor` permanently
/// `Cursor::Free` for every game in the workspace. The order of a frame is:
/// take the snapshot — which begins by clearing — ask the game what it wants
/// the pointer to do, tell the platform, and write back what actually took.
/// The write-back landed in the snapshot the *next* frame wiped, so nobody
/// could ever read it.
///
/// A game asking whether its lock had been granted got "no" for ever, whatever
/// the platform had actually done. `corvid_window/tests/cursor.rs` opens a real
/// window and measures all four modes, which is the check that was missing:
/// everything here is one side of a boundary whose other side is an operating
/// system.
#[test]
fn clearing_keeps_the_pointer_where_the_platform_put_it() {
    let mut input = Input::new(SETS);
    input.set_cursor(Cursor::Locked);
    input.clear();
    assert_eq!(
        input.cursor(),
        Cursor::Locked,
        "nothing unlocked the pointer, so it is still locked",
    );

    // And it is still a value the platform owns: a game changes it by asking
    // through `Controller::cursor` and reading the answer here, never by writing.
    input.set_cursor(Cursor::Free);
    input.clear();
    assert_eq!(input.cursor(), Cursor::Free);
}

/// A pointer mode is not an action, so it survives a change of set.
#[test]
fn activating_a_set_does_not_touch_the_pointer() {
    let mut input = Input::new(SETS);
    input.set_cursor(Cursor::Locked);
    input.activate(Playing::ID);
    assert_eq!(input.cursor(), Cursor::Locked);
}

/// The default is `Free`, in the type and in a snapshot both.
#[test]
fn the_default_is_free() {
    assert_eq!(Cursor::default(), Cursor::Free);
}

/// A borrowed descriptor list keeps the tests above honest about what `SETS`
/// is: the table `action_sets!` generated, not something this file made up.
#[test]
fn the_table_is_the_generated_one() {
    let table: &'static [SetDescriptor] = SETS;
    assert_eq!(table.len(), 1);
}
