//! What a run with no window, no adapter and no audio device is worth.
//!
//! The claim this file exists for is that such a run is a function of the
//! session and of nothing else — not of the wall clock, not of how fast the
//! machine is, not of which run it is. Every test below is an attempt to make
//! that false.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use std::{cell::RefCell, num::NonZeroU32, rc::Rc, sync::Arc, thread, time::Instant};

use common::{
    Action, Attending, Counting, Rules, Scratchpad, Tally, attendance,
    backstop::{joined, once},
    opening, resting, seat,
};
use corvid_app::{App, Game, Progress, Settings};
use corvid_behavior::{ExitCode, PlayerId};
use corvid_hash::Digest;
use corvid_replay::Opens;
use corvid_signal::channel;
use corvid_time::{Clock, Tick, TickSpan, Ticks};
use corvid_wire::golden::{DigestRow, check_digests};

/// How far the runs below play.
const TICKS: u64 = 40;

/// The seat this client submits for, which is the only seat these openings
/// have.
const SEAT: PlayerId = PlayerId(0);

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
fn the_sandbox_is_the_builder_lines_it_stands_for() {
    // `Counting::app()` is what `corvid_app::game!` generates over
    // `App::sandbox`, and what it is worth is that the run it builds is the one
    // written out beside it: the game's own opening, its own period, no device,
    // a directory of its own, and the defaults rather than whatever settings
    // file the machine this runs on happens to have. A sandbox that dropped any
    // of the five would be a run whose trace is not this one.
    let elsewhere = Scratchpad::new("sandbox");
    let sandboxed = Counting::app().for_ticks(Ticks(TICKS)).run().unwrap();
    let written_out = App::<Counting>::new()
        .opening(<Tally as Opens>::opening())
        .rate(<Counting as Game>::PERIOD)
        .headless()
        .state(elsewhere.path())
        .settings(Settings::default())
        .for_ticks(Ticks(TICKS))
        .run()
        .unwrap();

    assert_eq!(sandboxed.session.marks, written_out.session.marks);
    assert_eq!(sandboxed.session.log, written_out.session.log);
    assert_eq!(sandboxed.state, written_out.state);
    assert_eq!(sandboxed.session.last(), Tick(TICKS));
}

#[test]
fn two_sandboxes_do_not_share_a_directory() {
    // The claim the counter in `App::sandbox` is there for. Two calls made the
    // same way differ in exactly one thing — the state directory — because
    // every other setting a sandbox makes is a constant, so two renderings that
    // are not equal is that directory and nothing else. Without the counter
    // they would be the same path, and two tests running at once in this binary
    // would be two runs sharing a save slot.
    let one = format!("{:?}", Counting::app());
    let two = format!("{:?}", Counting::app());

    assert_ne!(one, two);
    // And that what differs is the sandbox root rather than something the
    // assertion above would also have caught.
    assert!(one.contains("corvid-sandbox-tally-"), "{one}");
    assert!(two.contains("corvid-sandbox-tally-"), "{two}");
}

#[test]
fn a_headless_run_is_reproducible() {
    let first = play(Rules::quiet());
    let second = play(Rules::quiet());

    // The trace is the comparison that matters, because it is what two peers
    // exchange and what a later build is diffed against.
    assert_eq!(first.session.marks, second.session.marks);
    assert!(!first.session.marks.is_empty());

    // And the rest of it, so that "the same trace" cannot be bought with a
    // trace that is short, or empty, or about a session that stopped somewhere
    // else.
    assert_eq!(first.session.log, second.session.log);
    assert_eq!(first.state, second.state);
    assert_eq!(first.session.last(), Tick(TICKS));
    assert_eq!(first.session.marks.len(), TICKS + 1);
}

#[test]
fn the_trace_of_a_fixed_run_is_frozen() {
    // Two runs agreeing with each other says the loop is a function of
    // something; it does not say of what. These literals say which function.
    // A change to the order the loop does its work in — a tick that ran before
    // its action was logged, a mark taken of the wrong state, a `look` moved to
    // before the tick instead of after it — moves this table and moves nothing
    // else in this file.
    //
    // A deliberate change here is a change to what every recorded session in
    // the workspace replays to. Paste the new column only after knowing which
    // of those it is.
    const TRACE: &[DigestRow<'_>] = &[
        // Moved when the level joined the origin in this mark — see
        // `Level::load`'s documentation for why it is always hashed now. Every
        // row below is a digest of a state and none of them moved with it,
        // which is what says the change reached the opening and nothing else.
        ("tick 0, the opening", 0x8794_c23c_9575_2fb3),
        ("tick 1, after a bump", 0xf05f_ef26_79e4_62da),
        ("tick 2", 0x94a6_a690_0b46_d6af),
        ("tick 3", 0x85d7_bce6_e290_ae71),
        (
            "tick 20, past the first simulated second",
            0x5a43_9c86_7884_780a,
        ),
        ("tick 40, the last", 0x601e_addb_f96d_fd8a),
    ];

    let run = play(Rules::quiet());
    let marks = run.session.marks;
    let sampled: Vec<u64> = [0, 1, 2, 3, 20, 40]
        .into_iter()
        .map(|tick| marks.get(Tick(tick)).unwrap().to_u64())
        .collect();
    check_digests("the tally's hash trace", TRACE, &sampled).unwrap();
}

#[test]
fn a_headless_run_does_not_read_the_wall_clock() {
    // `Tally::action` folds in the whole simulated seconds `look` has been
    // handed. Under the app's fake clock, fifteen ticks at fifteen hertz are
    // one such second, so from tick sixteen on the action sequence is shifted
    // by one against what the tick number alone would give.
    //
    // Ticks 17 and 18 are where that inverts an answer. With the fake clock the
    // phase at 17 is 17 + 1 = 18, a bump, and at 18 it is 19, not one. A loop
    // that handed `look` a real clock instead would pass microseconds per
    // frame, never reach a whole second inside forty ticks, and get the other
    // answer at both: 17 is not a bump and 18 is.
    let run = play(Rules::quiet());
    let log = &run.session.log;
    assert_eq!(log.get(Tick(17), SEAT), Some(&Action::Bump));
    assert_eq!(log.get(Tick(18), SEAT), Some(&Action::Idle));

    // And the two before the second lands, so that the pair above is read as a
    // shift rather than as an off-by-one anywhere: 15 is a bump because 15 is
    // divisible by three and no second has passed yet.
    assert_eq!(log.get(Tick(15), SEAT), Some(&Action::Bump));
    assert_eq!(log.get(Tick(16), SEAT), Some(&Action::Idle));
}

#[test]
fn a_headless_run_is_not_paced_by_the_time_it_simulates() {
    // Three hundred ticks at fifteen hertz is twenty seconds of game. The
    // margin is two orders of magnitude, so this is about whether the loop
    // waits for anything at all rather than about how fast this machine is.
    let rate = TickSpan::CRADLE;
    let simulated = rate.period() * 300;

    let started = Instant::now();
    let run = App::<Counting>::new()
        .headless()
        .rate(rate)
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(300))
        .run()
        .unwrap();
    let spent = started.elapsed();

    assert_eq!(run.session.last(), Tick(300));
    assert!(
        spent * 10 < simulated,
        "300 ticks of a twenty-second game took {spent:?}, which is not a run \
         that goes as fast as the processor allows",
    );
}

#[test]
fn the_input_the_app_was_given_reaches_action() {
    // The one thing a caller can say to `action` when nothing refills the
    // snapshot. `Tally::action` returns `Idle` while the rest button is held, so a
    // run given this snapshot never bumps and the tally never moves.
    let run = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .input(resting())
        .for_ticks(Ticks(TICKS))
        .run()
        .unwrap();

    assert_eq!(run.state.count, 0);
    assert!(
        run.session
            .log
            .row(Tick(0))
            .iter()
            .all(|action| *action == Action::Idle)
    );

    // The control: the same run without the snapshot does move, so the
    // assertion above is about the input rather than about a game that never
    // counts.
    assert!(play(Rules::quiet()).state.count > 0);
}

#[test]
fn asking_for_the_headless_backend_changes_nothing() {
    // `headless()` is a no-op today and says so. This is what would notice if
    // it stopped being one without anybody meaning it to.
    let with = play(Rules::quiet());
    let without = App::<Counting>::new()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(TICKS))
        .run()
        .unwrap();

    assert_eq!(with.session.marks, without.session.marks);
    assert_eq!(with.state, without.state);
    assert_eq!(with.exit, without.exit);
}

#[test]
fn until_stops_at_the_tick_whose_state_satisfied_it() {
    // The predicate is checked against the state a tick produced, so the run
    // stops with exactly as many ticks as it took to satisfy it and none after.
    let run = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        // The tick ceiling is a backstop rather than part of the claim: a
        // predicate that only watched the tally would never be satisfied by a
        // loop whose bug is that the tally stops moving, and a test that hangs
        // reports nothing.
        .until(|state: &Tally, at: Tick| state.count >= 6 || at >= Tick(TICKS))
        .run()
        .unwrap();

    assert_eq!(run.state.count, 6);
    assert_eq!(run.exit, ExitCode::SUCCESS);

    // Bumps land on ticks 0 and 3 with a step of three, so the sixth unit
    // arrives out of the tick at 3 and the state it produced is at 4.
    assert_eq!(run.state.now, Tick(4));
    assert_eq!(run.session.last(), Tick(4));
    assert_eq!(run.session.marks.len(), 5);
}

#[test]
fn for_ticks_of_zero_is_a_run_of_no_ticks() {
    // Documented, and the one case the predicate cannot answer: `until` is
    // checked *after* a tick, so a loop that only counted afterwards would have
    // simulated the tick it was asked not to. What answers zero is the count
    // being read on the way in as well, and this is the only test that can see
    // that read — every other `for_ticks` here would pass just as well without
    // it.
    let run = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(0))
        .run()
        .unwrap();

    assert_eq!(run.session.last(), Tick(0));
    assert_eq!(run.state, opening::<Tally>(Rules::quiet()).origin());
    // And the session says the same thing from the other side: no row in the
    // log, and the one mark a session opens with.
    assert_eq!(run.session.log.ticks(), 0);
    assert_eq!(run.session.marks.len(), 1);
}

#[test]
fn until_is_handed_the_tick_the_state_it_is_looking_at_is_at() {
    // Which tick a predicate is handed is a contract with an off-by-one in it
    // and nothing was reading it. `until` sees the state a tick *produced*, so
    // the number beside it is that state's own tick — one past the tick that
    // produced it — and not the tick that was asked for. A runtime that handed
    // over the asking tick instead would let every `for_ticks` in this file
    // pass and shift a caller's own predicate by one.
    let seen = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&seen);
    let run = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .until(move |state: &Tally, at: Tick| {
            recorded.borrow_mut().push((state.now, at));
            at >= Tick(4)
        })
        .run()
        .unwrap();

    // `Tally` counts its own ticks into its state, so the pairs say directly
    // that the two agree: the tick handed over is the state's, every time.
    assert_eq!(
        *seen.borrow(),
        vec![
            (Tick(1), Tick(1)),
            (Tick(2), Tick(2)),
            (Tick(3), Tick(3)),
            (Tick(4), Tick(4)),
        ],
    );
    assert_eq!(run.session.last(), Tick(4));
}

#[test]
fn for_ticks_counts_from_the_openings_first_tick_and_not_from_zero() {
    // The worked example in `for_ticks`'s own documentation: a session that
    // opens at tick five and is asked for ten ticks stops at fifteen. Every
    // other run in this workspace opens at zero, where counting from the
    // opening and counting from zero are the same number, so this is the only
    // place the sentence is an assertion.
    const OPENS_AT: Tick = Tick(5);
    const PLAYS: u64 = 10;

    let mut opening = opening::<Tally>(Rules::quiet());
    opening.first = OPENS_AT;
    // Through the handle rather than past it: an opening's origin is an `Arc`,
    // and nobody else is holding this one yet, so `make_mut` writes in place.
    // Stated explicitly, because this opening does *not* open on the default:
    // the point of the test is a session that starts partway through.
    let mut origin = opening.origin();
    Arc::make_mut(&mut origin).now = OPENS_AT;
    opening.origin = Some(origin);

    let run = App::<Counting>::new()
        .headless()
        .opening(opening)
        .for_ticks(Ticks(PLAYS))
        .run()
        .unwrap();

    assert_eq!(run.session.first(), OPENS_AT);
    assert_eq!(run.session.last(), Tick(OPENS_AT.0 + PLAYS));
    // And the game simulated ten ticks rather than stopping at whatever tick
    // ten happens to be, which is what a run counting from zero would do.
    assert_eq!(run.state.now, Tick(OPENS_AT.0 + PLAYS));
    assert_eq!(run.session.log.ticks(), PLAYS);
}

#[test]
fn the_clock_the_app_was_given_is_what_decides_how_often_a_tick_runs() {
    // The other direction of the wall-clock test: that one says a real clock
    // does not leak in, and this one says the given clock is what the loop is
    // actually paced by.
    //
    // A quarter of a period per reading is one owed tick per four readings, and
    // the loop displays once per reading. So four ticks cost sixteen displayed
    // frames — the number that would be four if the loop ignored its clock and
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
    // the two numbers coincide — so the sixteen above is about the clock rather
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
    // confirmation bit is the part a digest cannot see — an action equal to the
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
    // in question is the app's own. Substituting a constant — the cradle's
    // period, say, which is what `TickSpan::default` would hand over — is not
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
    // away and plays the recorded one — roster included. So the roster the seat
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
    // publishes `finished` would park this thread for good — and a test that
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
    // until the loop publishes over it — so this says a real mark arrived
    // rather than merely that the field has a type.
    assert_ne!(last.mark, Digest::ZERO);
}
