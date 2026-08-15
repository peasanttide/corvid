//! Writing a run down, reading it back, and finding the first tick the replay
//! stops agreeing with what ran.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{
    Climb, Climbing, Habit, idle, opening, opening_on_a_lossy_level, opening_with_a_lossy_origin,
};
use corvid_app::{App, Outcome};
use corvid_replay::Opening;
use corvid_test::{Diverged, Failed, What, replays_to_itself};
use corvid_time::Tick;
/// How far each run plays.
const TICKS: u64 = 20;

/// A run of `opening` for [`TICKS`] ticks.
fn played(opening: Opening<Climb>) -> Outcome<Climbing> {
    App::<Climbing>::new()
        .headless()
        .opening(opening)
        .input(idle())
        .until(|climb: &Climb, _| climb.now >= Tick(TICKS))
        .run()
        .unwrap()
}

/// The comparison's answer, or a failure that says which other thing went
/// wrong.
fn diverged(answer: Result<(), Failed>) -> Diverged {
    match answer {
        Ok(()) => panic!("the replay agreed"),
        Err(Failed::Diverged(diverged)) => diverged,
        Err(other) => panic!("the check did not get as far as a comparison: {other}"),
    }
}

#[test]
fn a_run_whose_every_part_survives_being_written_down_replays_to_itself() {
    replays_to_itself(&played(opening(Habit::Steady, 0, 0))).unwrap();
}

#[test]
fn a_level_a_capture_does_not_record_is_caught_before_a_single_tick_runs() {
    // The commonest shape of the bug this exists for: a `#[serde(skip)]` on a
    // field the tick reads.
    //
    // An opening mark over the origin alone would catch this at tick 1 -- the
    // first tick that *used* the level -- because a level that had not survived
    // the round trip would be invisible until it changed a state. The level is
    // in that mark, so the disagreement is at tick 0, before anything is
    // simulated, which is the whole point of hashing it:
    // "this file is not the one that was recorded" rather than "your states
    // stopped matching, go and find out why".
    let diverged = diverged(replays_to_itself(&played(opening_on_a_lossy_level())));

    assert_eq!(diverged.at, Tick(0));
    assert_eq!(diverged.agreed_through, None);
    let What::Marks { recorded, computed } = diverged.what else {
        panic!("{diverged}");
    };
    assert_ne!(recorded, computed);
}

#[test]
fn a_state_a_capture_does_not_record_is_caught_before_a_single_tick_runs() {
    // The other end of the same bug, and the reason the first tick is compared
    // at all rather than only the ticks a replay computes: when the *origin* is
    // what did not survive, the replay starts from the wrong state and there is
    // nothing before it that agreed.
    let diverged = diverged(replays_to_itself(&played(opening_with_a_lossy_origin())));

    assert_eq!(diverged.at, Tick(0));
    assert_eq!(diverged.agreed_through, None);
    assert!(matches!(diverged.what, What::Marks { .. }), "{diverged}");
}

#[test]
fn a_field_outside_the_digest_is_caught_after_every_digest_agreed() {
    // `Habit::Blind` moves a field this game's `Hash` does not absorb, and
    // the value it moves it to comes from a counter that keeps counting -- so the
    // replay computes a different one, every mark agrees, and the only thing
    // that can see it is the comparison that is not a digest.
    let diverged = diverged(replays_to_itself(&played(opening(Habit::Blind, 6, 0))));

    assert_eq!(diverged.at, Tick(TICKS));
    assert!(matches!(diverged.what, What::Unequal { .. }), "{diverged}");
}

#[test]
fn the_message_names_the_tick_and_what_moved() {
    // A divergence that is *not* at the first tick, so the message has both
    // halves to report: where it broke and how far it held. The lossy level is
    // no use for this any more -- it is caught at tick 0 now, where there is
    // nothing to have agreed through.
    let message = diverged(replays_to_itself(&played(opening(Habit::Blind, 6, 0)))).to_string();
    assert!(message.contains(&format!("tick {TICKS}")), "{message}");
    assert!(message.contains("agreed through"), "{message}");
}
