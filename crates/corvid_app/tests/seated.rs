//! Which seat a headless run plays, and the clock it counts ticks by.
//!
//! The seam against `headless.rs` is the subject: that file is about the run
//! being reproducible, and this is about who is in it and how fast it goes.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use std::{num::NonZeroU32, thread};

use common::{
    Attending, Counting, Rules, Scratchpad, Tally, attendance,
    backstop::{joined, once},
    opening, seat,
};
use corvid_app::{App, Progress};
use corvid_behavior::PlayerId;
use corvid_hash::Digest;
use corvid_signal::channel;
use corvid_time::{Clock, Tick, TickSpan, Ticks};

/// How far the runs below play.
const TICKS: u64 = 40;

/// A run of [`TICKS`] ticks of the honest game, with the rules given.
fn play(rules: Rules) -> corvid_app::Outcome<Counting> {
    App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(rules))
        .for_ticks(Ticks(TICKS))
        .run()
        .unwrap()
}

#[test]
fn a_run_with_no_opening_is_refused() {
    let refused = App::<Counting>::new().headless().run();
    assert!(matches!(refused, Err(corvid_app::Error::Unopened)));
}

#[test]
fn this_client_submits_for_the_seat_it_was_given_and_for_no_other() {
    // Every other fixture in this crate has a one-seat roster, which cannot see
    // this at all: with one seat, `self.seat` and `PlayerId(0)` are the same
    // number and the substitution passes the whole workspace. This roster has
    // three, and the client is the last of them.
    const MINE: PlayerId = PlayerId(2);

    let run = App::<Attending>::new()
        .headless()
        .opening(attendance(vec![seat(1000), seat(1001), seat(1002)]))
        .seat(MINE)
        .for_ticks(Ticks(3))
        .run()
        .unwrap();

    // What the *tick* saw: one column carries an action `action` built, and it
    // is the column this client was given.
    for (offset, roll) in run.state.rolls.iter().enumerate() {
        let mine: Vec<u16> = roll
            .seats
            .iter()
            .filter(|seen| seen.mine)
            .map(|seen| seen.id.0)
            .collect();
        assert_eq!(mine, [MINE.0], "tick {offset}");
    }

    // And what the *log* holds, which is what a replay reads: this seat's
    // column is confirmed at every tick and the other two are not. The
    // confirmation bit is the part a digest cannot see -- an action equal to the
    // default and an action nobody wrote are the same value and different
    // facts.
    let mut tick = run.session.first();
    while tick < run.session.last() {
        assert!(run.session.log.is_confirmed(tick, MINE), "{tick}");
        for empty in [PlayerId(0), PlayerId(1)] {
            assert!(
                !run.session.log.is_confirmed(tick, empty),
                "{tick} {empty:?}"
            );
        }
        tick = tick.next();
    }
}

#[test]
fn the_clock_decides_how_many_ticks_a_reading_owes() {
    // There is deliberately nothing here about the interpolation weight, which
    // is the thing a test of the clock most obviously reaches for.
    // `Controller::action` is handed no alpha and no frame: interpolation is
    // the renderer's and happens in a shader, so the weight between two states
    // reaches `Render::draw` and nowhere else. An action that read one would be
    // an action that depended on this machine's frame rate, which is exactly
    // what must never cross into a tick.
    //
    // What this asserts instead is the half that is about the clock: a clock
    // stepping one and a half periods owes one tick on its first reading and
    // two on its second, so four ticks arrive over three readings.
    let rate = TickSpan::from_hz(NonZeroU32::new(20).unwrap());
    let run = App::<Attending>::new()
        .headless()
        .rate(rate)
        .clock(Clock::stepping(rate.period() * 3 / 2))
        .opening(attendance(vec![seat(1000)]))
        .for_ticks(Ticks(4))
        .run()
        .unwrap();

    assert_eq!(run.state.rolls.len(), 4);
    assert_eq!(run.session.last(), Tick(4));

    // And every column is this client's, whatever the clock did.
    assert!(run.state.rolls.iter().all(|roll| roll.seats[0].mine));
}

#[test]
fn the_default_clock_is_the_rate_the_app_was_given() {
    // The documented default is `Clock::stepping(rate.period())`, and the rate
    // in question is the app's own. Substituting a constant -- the cradle's
    // period, say, which is what `TickSpan::default` would hand over -- is not
    // visible at the default rate and is visible here, because a run at twenty
    // hertz driven by a fifteen-hertz clock owes two ticks on every third
    // reading rather than one on every reading.
    let rate = TickSpan::from_hz(NonZeroU32::new(20).unwrap());
    assert_ne!(rate.period(), TickSpan::CRADLE.period());

    let defaulted = frames_of(
        App::<Counting>::new()
            .headless()
            .rate(rate)
            .opening(opening::<Tally>(Rules::quiet())),
    );
    let spelled_out = frames_of(
        App::<Counting>::new()
            .headless()
            .rate(rate)
            .clock(Clock::stepping(rate.period()))
            .opening(opening::<Tally>(Rules::quiet())),
    );
    assert_eq!(defaulted, spelled_out);

    // And the substitution really is a different run, so the equality above is
    // an assertion rather than a coincidence about a loop that displays once
    // per tick whatever it is driven by.
    let elsewhere = frames_of(
        App::<Counting>::new()
            .headless()
            .rate(rate)
            .clock(Clock::stepping(TickSpan::CRADLE.period()))
            .opening(opening::<Tally>(Rules::quiet())),
    );
    assert_ne!(defaulted, elsewhere);
}

/// How many frames a run displays, which is the thing a clock decides and a
/// tick count does not.
fn frames_of(app: App<Counting>) -> u64 {
    let (emit, watch) = channel(
        "frames",
        Progress {
            tick: Tick::ZERO,
            mark: Digest::ZERO,
            frames: 0,
            finished: false,
        },
    );
    let run = app.progress(emit).for_ticks(Ticks(8)).run().unwrap();
    assert_eq!(run.session.last(), Tick(8));
    watch.get().frames
}

#[test]
fn a_seat_the_roster_does_not_have_is_refused() {
    // The roster these openings carry has one seat. Submitting for the second
    // would record this client's actions nowhere, and a replay of the session
    // would be a replay of a run in which nobody did anything.
    let refused = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .seat(PlayerId(1))
        .for_ticks(Ticks(1))
        .run();

    match refused {
        Err(corvid_app::Error::Seat { seat, seats }) => {
            assert_eq!(seat, PlayerId(1));
            assert_eq!(seats, 1);
        }
        other => panic!("a seat outside the roster was accepted: {other:?}"),
    }
}

#[test]
fn a_resumed_run_checks_its_seat_against_the_roster_it_is_resuming() {
    // A run that opens on a recorded session throws the game's fresh opening
    // away and plays the recorded one -- roster included. So the roster the seat
    // has to be in is that one, and a seat checked against the opening that was
    // discarded is a seat checked against a roster nobody will play with.
    let scratchpad = Scratchpad::new("resumed-seat");
    let capture = scratchpad.path().join("capture");
    App::<Attending>::new()
        .headless()
        .opening(attendance(vec![seat(1000), seat(1001)]))
        .capture(&capture)
        .for_ticks(Ticks(4))
        .run()
        .unwrap();

    // Seat three is in the fresh roster of four and not in the recorded roster
    // of two. Refused, and refused as a seat rather than a hundred ticks later
    // as a log that would not take a write.
    let refused = App::<Attending>::new()
        .headless()
        .opening(attendance(vec![
            seat(1000),
            seat(1001),
            seat(1002),
            seat(1003),
        ]))
        .replay(capture.join("session"))
        .seat(PlayerId(3))
        .for_ticks(Ticks(1))
        .run();

    match refused {
        Err(corvid_app::Error::Seat { seat, seats }) => {
            assert_eq!(seat, PlayerId(3));
            assert_eq!(seats, 2, "the roster reported is the one being resumed");
        }
        other => panic!("a seat outside the resumed roster was accepted: {other:?}"),
    }
}

#[test]
fn the_session_a_run_leaves_is_internally_consistent() {
    // `Session::check` is the comparison `Session::load` makes on a capture.
    // A run whose own session did not pass it would be a run that wrote a
    // capture nothing could load, which the capture tests would find one file
    // later than this does.
    let run = play(Rules::quiet());
    run.session.check().unwrap();
    assert_eq!(run.session.marks.first(), run.session.first());
    assert_eq!(
        run.session.marks.get(run.session.last()),
        Some(Digest::from_u64(corvid_hash::digest(&run.state).to_u64())),
    );
}

#[test]
fn a_run_reports_where_it_has_got_to_while_it_is_still_running() {
    // `run` blocks the thread it was called on, so a signal is the only thing
    // a supervisor, a progress bar or a watchdog has to look at. The run goes
    // on its own thread here for exactly that reason.
    let (emit, watch) = channel(
        "run",
        Progress {
            tick: Tick::ZERO,
            mark: Digest::ZERO,
            frames: 0,
            finished: false,
        },
    );
    let mut seen = watch.seen_now();

    let playing = thread::spawn(move || {
        App::<Counting>::new()
            .headless()
            .opening(opening::<Tally>(Rules::quiet()))
            .progress(emit)
            .for_ticks(Ticks(TICKS))
            .run()
            .unwrap()
            .session
            .last()
    });

    // A latest-value cell drops what nobody read, so which of the intermediate
    // publications this thread sees is a race and is not asserted on. What is
    // asserted is that every one it does see belongs to this run and that they
    // arrive in order.
    //
    // Neither wait below is part of the claim; both are there so that a runtime
    // which stopped publishing fails this test instead of hanging it.
    // `blocking_wait` blocks until the *next* publication, so a run that never
    // publishes `finished` would park this thread for good -- and a test that
    // hangs reports nothing: no name, no assertion, and a CI job killed by its
    // own timeout an hour later. The join is the same hazard one line down,
    // since a run that never returns is a `join` that never returns.
    let mut watched = Vec::new();
    let mut last = None;
    once("the run publishing a finished progress", || {
        if let Some(progress) = watch.changed_since(&mut seen) {
            watched.push(progress.tick);
            if progress.finished {
                last = Some(*progress);
            }
        }
        last.is_some()
    });
    let last = last.unwrap();
    let reached = joined("the run this test is watching", playing);

    assert!(
        watched.windows(2).all(|pair| pair[0] <= pair[1]),
        "{watched:?}"
    );
    assert_eq!(last.tick, Tick(TICKS));
    assert_eq!(last.tick, reached);
    assert_eq!(last.frames, TICKS);
    // The seed this channel was opened with, which is what the field holds
    // until the loop publishes over it -- so this says a real mark arrived
    // rather than merely that the field has a type.
    assert_ne!(last.mark, Digest::ZERO);
}
