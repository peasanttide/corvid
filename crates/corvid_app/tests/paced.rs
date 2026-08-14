//! What decides how many ticks a headless run computes.
//!
//! The seam against `headless.rs` is the clock: that file is about a run being
//! the same run twice, and this is about how fast one goes.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use std::num::NonZeroU32;

use common::{Counting, Rules, Tally, opening};
use corvid_app::{App, Progress};
use corvid_hash::Digest;
use corvid_signal::channel;
use corvid_time::{Clock, Tick, TickSpan, Ticks};

#[test]
fn the_clock_the_app_was_given_is_what_decides_how_often_a_tick_runs() {
    // The other direction of the wall-clock test: that one says a real clock
    // does not leak in, and this one says the given clock is what the loop is
    // actually paced by.
    //
    // A quarter of a period per reading is one owed tick per four readings, and
    // the loop displays once per reading. So four ticks cost sixteen displayed
    // frames -- the number that would be four if the loop ignored its clock and
    // ticked once per iteration, and four again if it ignored the clock the
    // other way and displayed once per tick.
    //
    // Twenty hertz rather than the cradle's fifteen because fifty milliseconds
    // divide into four whole quarters and a fifteenth of a second does not. At
    // the cradle rate the quarter is truncated by half a nanosecond a reading,
    // the accumulator falls two nanoseconds short every fourth one, and the
    // seventeenth reading is what delivers the fourth tick. That is the step's
    // arithmetic behaving exactly as `corvid_time` documents rather than
    // anything about this loop, so the rate here is chosen to leave it out.
    let rate = TickSpan::from_hz(NonZeroU32::new(20).unwrap());
    let (emit, watch) = channel(
        "quarter",
        Progress {
            tick: Tick::ZERO,
            mark: Digest::ZERO,
            frames: 0,
            finished: false,
        },
    );

    let run = App::<Counting>::new()
        .headless()
        .rate(rate)
        .clock(Clock::stepping(rate.period() / 4))
        .opening(opening::<Tally>(Rules::quiet()))
        .progress(emit)
        .for_ticks(Ticks(4))
        .run()
        .unwrap();

    assert_eq!(run.session.last(), Tick(4));
    assert_eq!(watch.get().frames, 16);

    // And the control at the default rate, where one reading is one tick and
    // the two numbers coincide -- so the sixteen above is about the clock rather
    // than about a loop that displays four times whatever it does.
    let (emit, watch) = channel(
        "whole",
        Progress {
            tick: Tick::ZERO,
            mark: Digest::ZERO,
            frames: 0,
            finished: false,
        },
    );
    let run = App::<Counting>::new()
        .headless()
        .rate(rate)
        .opening(opening::<Tally>(Rules::quiet()))
        .progress(emit)
        .for_ticks(Ticks(4))
        .run()
        .unwrap();

    assert_eq!(run.session.last(), Tick(4));
    assert_eq!(watch.get().frames, 4);
}
