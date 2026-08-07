//! The client-local pause: what stops, what carries on, and what the clock
//! does with the time nobody simulated.
//!
//! [`Present::simulating`](corvid_control::Controller::simulating) is asked once per
//! reading of the clock, before the ticks that reading owes, and the game here
//! answers it out of a field its `look` writes — which is the whole shape of
//! the feature: a pause is a property of what one player is looking at, so it
//! lives in the view and nothing about it goes on the wire.
//!
//! The three claims below are the three that can be got wrong.
//!
//! **The ticks stop.** A run that pauses for five displayed frames simulates
//! exactly as many ticks as one that never paused, and takes five more frames
//! to do it.
//!
//! **The clock does not bank the paused time.** This is the one that costs a
//! player something when it is wrong: an accumulator that went on filling while
//! nothing was simulated hands the frame after the pause a backlog, and a
//! ten-second pause is followed by the catch-up ceiling's worth of ticks all at
//! once. The test for it makes the pause last ten seconds of wall clock and
//! asserts the run needs the same number of frames as one whose pause lasted
//! five milliseconds.
//!
//! **Nothing else stops.** `look` and `hear` run on every paused frame and the
//! backend is handed every one of them, because the pause screen has to be
//! drawn and navigated while the world behind it holds still.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use std::time::Duration;

use common::{Ears, Hands, Holding, Painted, Rules, Tally, action, opening};
use corvid_app::{App, Outcome, Progress};
use corvid_control::Controller;
use corvid_hash::{Digest, digest};
use corvid_input::{Digital, Input};
use corvid_signal::channel;
use corvid_time::{Elapsed, Tick, TickSpan};

/// How far every run below plays.
const TICKS: u64 = 12;

/// The tick the pause starts on.
const PAUSE_AT: Tick = Tick(4);

/// How many displayed frames it lasts.
const PAUSE_FOR: u64 = 5;

/// A clock that hands out one tick period per reading, plus one long stall.
///
/// [`Fake`](corvid_time::Fake) can queue an interval and cannot be reached once
/// it has been boxed into an [`App`], and what these tests need is a stall that
/// lands on a chosen reading — inside the pause, where the whole question is
/// what the accumulator does with time nobody simulated.
#[derive(Debug)]
struct Stalling {
    /// The period handed out on an ordinary reading.
    period: Duration,
    /// Which reading stalls, counting from one.
    on: u64,
    /// How long that reading lasts instead.
    stall: Duration,
    /// How many readings have been taken.
    reads: u64,
}

impl Stalling {
    /// A clock that stalls for `stall` on the `on`th reading.
    const fn new(period: Duration, on: u64, stall: Duration) -> Self {
        Self {
            period,
            on,
            stall,
            reads: 0,
        }
    }
}

impl Elapsed for Stalling {
    fn elapsed(&mut self) -> Duration {
        self.reads += 1;
        if self.reads == self.on {
            self.stall
        } else {
            self.period
        }
    }
}

/// A snapshot with the rest key held for the whole run.
///
/// [`Tally::intend`](common::Tally) otherwise mixes the wall time `look` has
/// been handed into its action, deliberately, so that a clock the app was not
/// given has a route into the log — which `tests/headless.rs` is what walks.
/// Here that route is exactly the thing in the way: a paused run displays more
/// frames than an unpaused one, so it accumulates more of that clock and would
/// submit different actions for reasons that have nothing to do with the pause.
/// Resting takes the display out of the action and leaves the state a function
/// of the tick count, which is what these tests are comparing.
fn resting() -> Input {
    let mut input = Input::new(action::SETS);
    input.set_digital(action::REST, Digital::HELD);
    input
}

/// The controller's settings, pausing at [`PAUSE_AT`] for [`PAUSE_FOR`] frames.
///
/// A `Config` rather than `Rules`: a pause is one machine's, and `Rules` is the
/// half of a game's tuning every peer has to agree on.
const fn pausing() -> Holding {
    Holding {
        pause_at: Some(PAUSE_AT),
        pause_for: PAUSE_FOR,
    }
}

/// The settings for a run that never pauses.
const fn never() -> Holding {
    Holding {
        pause_at: None,
        pause_for: 0,
    }
}

/// A run of [`TICKS`] ticks under `rules`, with `stall` on the `on`th reading
/// of the clock, and where it got to.
///
/// The progress watch is the only way in from outside a run: it carries the
/// tick the run reached and how many frames the backend was handed, which
/// between them are exactly what a pause is supposed to move apart.
fn play(holding: Holding, on: u64, stall: Duration) -> (Outcome<Tally>, Progress) {
    let rate = TickSpan::CRADLE;
    let (emitter, watch) = channel(
        "pause",
        Progress {
            tick: Tick::ZERO,
            mark: Digest::ZERO,
            frames: 0,
            finished: false,
        },
    );
    let outcome = App::<Tally, Hands, Painted, Ears>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .controls(holding)
        .input(resting())
        .clock(Stalling::new(rate.period(), on, stall))
        .rate(rate)
        .progress(emitter)
        .for_ticks(TICKS)
        .run()
        .unwrap();
    let progress = *watch.get();
    (outcome, progress)
}

/// A run with no stall at all.
fn played(holding: Holding) -> (Outcome<Tally>, Progress) {
    // Reading zero is a reading that never happens, so nothing stalls.
    play(holding, 0, Duration::ZERO)
}

#[test]
fn a_pause_stops_the_ticks_and_nothing_else() {
    let (running, ran) = played(never());
    let (paused, held) = played(pausing());

    // The same ticks, and the same states: a pause changes what a run does with
    // its clock and nothing at all about what it computes.
    assert_eq!(running.session.last(), Tick(TICKS));
    assert_eq!(paused.session.last(), Tick(TICKS));
    assert_eq!(digest(&paused.state), digest(&running.state));

    // And the frames the pause cost, which is the other half of the claim: they
    // were displayed rather than skipped, because a pause screen has to be
    // drawn and navigated while the world behind it holds still.
    assert_eq!(
        held.frames,
        ran.frames + PAUSE_FOR,
        "a run that paused for {PAUSE_FOR} frames displayed {} of them against \
         {} for a run that did not",
        held.frames,
        ran.frames,
    );
}

#[test]
fn the_clock_does_not_bank_the_time_a_pause_spent() {
    // The reading the stall lands on. The pause begins on the frame that
    // follows the tick at `PAUSE_AT`, and one reading is one tick until then,
    // so this is the first reading with no tick in it.
    let inside = PAUSE_AT.0 + 1;

    let (brief, quick) = play(pausing(), inside, Duration::from_millis(5));
    let (long, slow) = play(pausing(), inside, Duration::from_secs(10));

    // Ten seconds of wall clock passed with the simulation stopped, and it cost
    // the run nothing: the same ticks, in the same number of displayed frames,
    // as a pause that lasted five milliseconds.
    //
    // An accumulator that had gone on filling would have owed a hundred and
    // fifty ticks at `CRADLE` on the frame the pause ended, delivered the
    // catch-up ceiling of eight of them at once and counted the rest as
    // dropped — so the long run would have reached `TICKS` seven readings
    // earlier than the brief one, and this is the equality that says it did
    // not.
    assert_eq!(slow.frames, quick.frames);
    assert_eq!(long.session.last(), Tick(TICKS));
    assert_eq!(brief.session.last(), Tick(TICKS));
    assert_eq!(digest(&long.state), digest(&brief.state));

    // The same stall outside a pause is the case that *does* catch up, which is
    // what makes the assertion above about the pause rather than about the
    // clock being ignored everywhere.
    let (stalled, spent) = play(never(), inside, Duration::from_secs(10));
    let (steady, plain) = played(never());
    assert!(
        spent.frames < plain.frames,
        "a ten-second stall with nothing paused delivered no catch-up at all",
    );
    assert_eq!(stalled.session.last(), Tick(TICKS));
    assert_eq!(digest(&stalled.state), digest(&steady.state));
}

#[test]
fn a_view_nobody_paused_reports_a_running_simulation() {
    // The state a run opens in, before any `look` has happened: `View::default`
    // is what the runtime builds, and a game whose default view read as paused
    // would be a game that never simulated its first tick.
    assert!(<Hands as Controller<Tally>>::new(never()).simulating());

    // And the same from the other end: a run whose rules ask for no pause
    // reaches every tick it was asked for.
    let (outcome, _) = played(never());
    assert_eq!(outcome.session.last(), Tick(TICKS));
}
