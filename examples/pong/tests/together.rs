//! Multiplayer without a second machine, tested.
//!
//! [`pong::rally::together`] plays both seats in this process -- one through the
//! runtime, one as a whole [`Peer`](corvid_lockstep::Peer) on a thread, over a
//! link with latency and loss in it -- so it is worth being sure it is two peers
//! rather than a single-seat run with a picture of an opponent in it. What makes
//! that checkable is that the mode hands its run back: a session with another
//! machine in it heard datagrams, and one without heard none.
//!
//! The window is the one part of it this cannot open -- a test harness runs on a
//! worker thread and an event loop may only be built off the main one on X11
//! and Wayland -- so the windowed arm follows `examples/hello`'s
//! `tests/windowed.rs` and is skipped where that is not true.

#![cfg(feature = "net")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "every test here returns a `Result` so that a failure reaches for `?` rather than unwrapping, and asserts as well -- a failed assertion in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::print_stderr,
    reason = "a test that is skipped has to say so where a person running the suite will see it, and a tracing event needs a subscriber the harness does not install"
)]

use corvid::PlayerId;
use pong::rally::together;

/// Whatever the test needs to say went wrong.
type Fallible = Result<(), Box<dyn std::error::Error>>;

/// How long the run plays. Past the first serve, so the ball is in play and the
/// opponent has something to chase -- and no longer than that, because this mode
/// keeps real time and every tick of it is a thirtieth of a second of test.
const TICKS: u64 = 150;

/// The variable that asks for a test that opens a window.
///
/// `cargo test` opens none without it. A suite that opened windows steals focus
/// from whatever the person running it was typing into, cannot be run while
/// doing something else, and behaves differently on a build machine with no
/// display than on the desk it was written at -- so the window-opening arm is
/// something to ask for rather than something to be ambushed by. Everything
/// this file claims that can be checked without one is checked without one.
const ASKED: &str = "CORVID_WINDOWED_TESTS";

/// Whether a window was asked for.
fn windows_wanted() -> bool {
    std::env::var_os(ASKED).is_some_and(|value| !value.is_empty())
}

/// Both seats really play, and the run is really a session between them.
///
/// The counters are the evidence. A run that had quietly become a single-seat
/// one would still finish, still draw and still print a score -- and would have
/// heard nothing from anybody, which is what this asserts it did not.
#[test]
fn both_seats_play_and_the_session_is_shared() -> Fallible {
    let outcome = together(PlayerId(0), Some(TICKS), false)?;

    assert!(
        outcome.state.now.0 >= TICKS - 20,
        "the run stopped at tick {} of {TICKS}, which is a session that stalled",
        outcome.state.now.0,
    );
    assert!(
        outcome.traffic.heard > TICKS / 2,
        "this seat heard {} datagrams in {TICKS} ticks, which is not an opponent",
        outcome.traffic.heard,
    );
    assert!(
        outcome.traffic.sent > TICKS / 2,
        "this seat sent {} datagrams in {TICKS} ticks",
        outcome.traffic.sent,
    );

    // And the other seat moved, which is the difference between an opponent and
    // an empty chair: nothing in this process touches seat one's paddle except
    // the peer on the other end of the link.
    assert_ne!(
        outcome.state.paddles[1].at,
        pong::origin().paddles[1].at,
        "the opponent's paddle never moved",
    );
    Ok(())
}

/// The same, with a window, where a window can be opened from a test.
///
/// It is the same call with one argument changed, so what this adds over the
/// test above is exactly the window -- which is the part that cannot be checked
/// any other way and the part a player actually uses.
#[test]
#[cfg_attr(
    not(all(unix, not(target_vendor = "apple"), not(target_os = "android"))),
    ignore = "an event loop may only be built off the main thread on X11 and Wayland"
)]
fn the_windowed_arm_plays_the_same_session() {
    if !windows_wanted() {
        eprintln!("skipped: this test opens a window; set {ASKED}=1 to run it");
        return;
    }
    match together(PlayerId(0), Some(TICKS), true) {
        Ok(outcome) => {
            assert!(outcome.traffic.heard > TICKS / 2);
            assert!(outcome.state.now.0 >= TICKS - 20);
        }
        Err(corvid::Error::NoWindow(why)) => {
            eprintln!("skipped: this machine has no window to open ({why})");
        }
        Err(corvid::Error::Drew(why)) => {
            eprintln!("skipped: this machine has no adapter to draw with ({why})");
        }
        Err(why) => panic!("the windowed run failed for a reason that is not the machine: {why}"),
    }
}
