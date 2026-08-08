//! What a client that watches a seat without playing it does.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Action, Attending, Bare, Botted, Counting, Rules, Tally, attendance, opening, seat};
use corvid_app::{App, Arguments};
use corvid_behavior::PlayerId;
use corvid_time::{Tick, Ticks};

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
        .for_ticks(Ticks(TICKS))
        .spectating()
        .run()
        .expect("a spectating run");

    let idle = App::<Bare>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(TICKS))
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
        .for_ticks(Ticks(TICKS))
        .spectating()
        .run()
        .expect("a spectating run");

    let played = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(TICKS))
        .run()
        .expect("a played run");

    assert_eq!(watched.state.count, 0, "a spectator moved the tally");
    assert_ne!(played.state.count, 0, "the played run never bumped");
}

/// A spectator watches the seat it was told to, and `--spectator` is the same
/// two calls the builder makes.
///
/// The roster here has one seat, so watching the second is
/// [`Error::Seat`](corvid_app::Error::Seat) — which is the observation: a
/// `spectating` that always watched the roster's first seat would start this
/// run happily, and an operator who asked to watch the other player would be
/// watching themselves.
#[test]
fn a_spectator_watches_the_seat_it_named() {
    let why = App::<Bare>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(1))
        .seat(PlayerId(1))
        .spectating()
        .run()
        .expect_err("a one-seat roster has no second seat to watch");
    assert!(
        matches!(why, corvid_app::Error::Seat { seat, .. } if seat == PlayerId(1)),
        "{why:?}",
    );

    let told = App::<Bare>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(1))
        .arguments(Arguments::parse(["--spectator", "--seat", "1"]).expect("two flags"))
        .run()
        .expect_err("the command line says the same thing");
    assert!(
        matches!(told, corvid_app::Error::Seat { seat, .. } if seat == PlayerId(1)),
        "{told:?}",
    );
}

/// And the positive half, which no refusal can make: on a roster that *has* the
/// named seat, the run starts and the seat it watches is the one it was told.
///
/// Every other case in this file is a refusal or an empty roster, so a
/// `spectating` that quietly watched the roster's first seat would pass all of
/// them. Two seats and one bot is what separates the two answers. The bot skips
/// the seat this client *plays*, and a spectator plays none — so seat zero is
/// the bot's, seat one is watched by nobody's controller and submitted for by
/// nothing, and the run reaches the end rather than being refused. A watcher
/// pinned to seat zero would be watching the seat the bot is in.
///
/// The seat is the watched one for the bound as well: seat two on the same
/// roster is [`Error::Seat`](corvid_app::Error::Seat) naming two, which is the
/// same value travelling to the same check.
#[test]
fn a_spectator_watches_a_named_seat_of_a_roster_that_has_it() {
    let mut roster = opening::<Tally>(Rules::quiet());
    roster.roster.push(seat(1001));

    let watched = App::<Botted>::new()
        .headless()
        .opening(roster.clone())
        .for_ticks(Ticks(TICKS))
        .seat(PlayerId(1))
        .spectating()
        .bots(1)
        .run()
        .expect("a two-seat roster has a second seat to watch");

    // The bot's, because a spectator claims nothing for it to skip.
    assert_eq!(
        watched.session.log.get(Tick::ZERO, PlayerId(0)).copied(),
        Some(Action::Bump),
    );
    // The watched one: a real seat, with a column of its own, that this client
    // neither plays nor lets the single bot reach.
    assert_eq!(
        watched.session.log.get(Tick::ZERO, PlayerId(1)).copied(),
        Some(Action::Idle),
    );

    let past = App::<Botted>::new()
        .headless()
        .opening(roster)
        .for_ticks(Ticks(1))
        .seat(PlayerId(2))
        .spectating()
        .run()
        .expect_err("a two-seat roster has no third seat to watch");
    assert!(
        matches!(past, corvid_app::Error::Seat { seat, seats: 2 } if seat == PlayerId(2)),
        "{past:?}",
    );
}

/// A roster with nobody in it is refused before the seat is looked at, because
/// "which of the zero seats is this" has no answer worth reporting.
#[test]
fn a_roster_with_no_seats_has_nothing_to_watch() {
    let why = App::<Attending>::new()
        .headless()
        .opening(attendance(Vec::new()))
        .for_ticks(Ticks(1))
        .spectating()
        .run()
        .expect_err("a roster with no seats");

    assert!(matches!(why, corvid_app::Error::NoSeats), "{why:?}");
}
