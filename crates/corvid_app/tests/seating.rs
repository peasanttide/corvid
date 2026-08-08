//! What a client that watches a seat without playing it does.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Attending, Bare, Counting, Rules, Tally, attendance, opening};
use corvid_app::App;

/// How far the runs below play.
const TICKS: u64 = 30;

/// The digests are the assertion: a spectator submits nothing, and a seat
/// nobody submits for holds the idle action — so the session a spectator
/// watches is the session an idle player would have produced.
///
/// [`Bare`] is the game because its controller is `()`, which answers
/// `Action::default()` for ever. That is what makes the two runs comparable at
/// all: the claim is about the *log*, and a controller with an opinion would
/// have the played run bumping the tally where the spectating one did not.
#[test]
fn a_spectator_submits_nothing() {
    let watched = App::<Bare>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(TICKS)
        .spectating()
        .run()
        .expect("a spectating run");

    let idle = App::<Bare>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(TICKS)
        .run()
        .expect("a played run");

    // Before the comparison, because two empty traces are also equal: a run
    // that refused to tick would satisfy the assertion below and say nothing.
    // A trace holds the opening's mark and one per tick.
    assert_eq!(watched.session.marks.len(), TICKS + 1);

    assert_eq!(watched.session.marks, idle.session.marks);
}

/// And the other half of that claim, which the equality above cannot make on
/// its own: a controller that *does* have an opinion is not asked for one.
///
/// [`Counting`]'s controller bumps the tally on one tick in three, so a played
/// run and a spectating run of it reach different states. If `spectating` only
/// stopped the *write* and still called `action`, this would still pass — but
/// if it stopped neither, the test above would pass by accident and this one
/// would fail.
#[test]
fn a_spectator_does_not_play_the_seat_it_watches() {
    let watched = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(TICKS)
        .spectating()
        .run()
        .expect("a spectating run");

    let played = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(TICKS)
        .run()
        .expect("a played run");

    assert_eq!(watched.state.count, 0, "a spectator moved the tally");
    assert_ne!(played.state.count, 0, "the played run never bumped");
}

/// A roster with nobody in it is refused before the seat is looked at, because
/// "which of the zero seats is this" has no answer worth reporting.
#[test]
fn a_roster_with_no_seats_has_nothing_to_watch() {
    let why = App::<Attending>::new()
        .headless()
        .opening(attendance(Vec::new()))
        .for_ticks(1)
        .spectating()
        .run()
        .expect_err("a roster with no seats");

    assert!(matches!(why, corvid_app::Error::NoSeats), "{why:?}");
}
