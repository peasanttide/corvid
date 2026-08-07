#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

//! The digests this game produced before the contracts were rewritten.
//!
//! Not a golden anybody chose. These are what `pong --headless --bot` printed on
//! 2026-08-06, on the commit before `State`, `Controller`, `Render` and
//! `Auralizer` replaced the marker-type chain, and the whole claim of that
//! rewrite is that **they do not move**.
//!
//! So a failure here is never a test that needs updating. It is the simulation
//! having changed — some arithmetic reordered, some field's width altered, some
//! tick reading something it did not read before — and the number is the only
//! evidence that would ever have said so. After the ring is rewritten there is
//! nothing left to compare against, which is why this file exists before the
//! rewrite rather than after it.
//!
//! The scripted paddle is a function of the tick alone, so nothing here depends
//! on a clock, a display or a scheduler: the same two numbers come out of a
//! debug build, a release build and a machine with one core.

use corvid::{App, Digital, Input, PlayerId, Tick};
use pong::{Hands, RATE, Table};

/// How far back the compared digest is taken from.
///
/// The same twenty ticks `main.rs` reports at, and for the same reason: the
/// newest few ticks of a peer's state were simulated partly from predictions of
/// what another machine did, so they are not a number two runs can be held to.
/// A single-seat run predicts nothing, but the constant is shared so that the
/// number this file asserts on is the number the binary prints.
const SETTLED: u64 = 20;

/// The paddle `--bot` drives, as a function of the tick alone.
///
/// Copied from `main.rs` rather than shared with it, deliberately: this test's
/// whole job is to be a fixed point, and a fixed point that moves when the
/// binary is refactored is not one.
fn scripted(seat: u16) -> impl FnMut(Tick) -> Input {
    move |at: Tick| {
        let mut input = Input::new(pong::action::SETS);
        let period = if seat == 0 { 17 } else { 11 };
        let held = if at.0 % period < period / 2 {
            pong::action::UP
        } else {
            pong::action::DOWN
        };
        input.set_digital(held, Digital::HELD);
        input
    }
}

/// Plays `ticks` of pong with the scripted paddle, and answers the settled
/// digest and the score.
fn play(ticks: u64) -> (u64, [u16; 2]) {
    let outcome = App::<Table, Hands>::new()
        .opening(pong::opening())
        .rate(RATE)
        .seat(PlayerId(0))
        .input(Input::new(pong::action::SETS))
        .inputs(scripted(0))
        .headless()
        .for_ticks(ticks)
        .run()
        .expect("this run opens no window, binds no socket and touches no disk");

    let settled = Tick(outcome.state.now.0.saturating_sub(SETTLED));
    let mark = outcome
        .session
        .marks
        .get(settled)
        .expect("a tick twenty back is inside the retention window every run keeps")
        .to_u64();
    (mark, outcome.state.scores)
}

#[test]
fn six_hundred_ticks_digest_to_what_they_always_have() {
    let (mark, scores) = play(600);
    assert_eq!(
        mark, 0x2aa3_8102_b9f9_b99e,
        "the simulation moved: this is the digest of tick 580 of a 600-tick \
         scripted run, and it is not what it was on 2026-08-06",
    );
    assert_eq!(scores, [5, 0], "and the game it played is a different game");
}

#[test]
fn three_hundred_ticks_digest_to_what_they_always_have() {
    let (mark, scores) = play(300);
    assert_eq!(
        mark, 0xbd7f_7f60_02dd_d534,
        "the simulation moved: this is the digest of tick 280 of a 300-tick \
         scripted run, and it is not what it was on 2026-08-06",
    );
    assert_eq!(scores, [4, 0], "and the game it played is a different game");
}

/// Two lengths, and the shorter one is a prefix of the longer.
///
/// Worth asserting separately from the two digests above, because a run that
/// produced the right number at 600 ticks and the wrong one at 300 would be a
/// simulation whose history depends on how long it is going to be played for —
/// which is the failure a snapshot ring or a retention window causes, and not
/// one either digest alone would name.
#[test]
fn the_shorter_run_is_the_longer_one_stopped_early() {
    let (_, short) = play(300);
    let (_, long) = play(600);
    assert!(
        short[0] <= long[0] && short[1] <= long[1],
        "seat scores went backwards between a 300-tick run and a 600-tick one, \
         so the two runs did not play the same game",
    );
}
