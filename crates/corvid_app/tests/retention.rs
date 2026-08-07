//! What a run keeps, what it lets go of, and that letting go changes nothing
//! about what it computed.
//!
//! The shape of every test here is the same: run the same opening twice, once
//! keeping everything and once keeping a window, and compare. A bounded run that
//! agreed with itself would prove nothing — it is the unbounded run beside it
//! that says the arithmetic did not move.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Action, Ears, Hands, Painted, Rules, Scratchpad, Tally, mark, opening, seat};
use corvid_app::{App, Outcome, Progress, Retention};
use corvid_behavior::PlayerId;
use corvid_replay::Unreachable;
use corvid_replay::{Session, Snapshots};
use corvid_signal::channel;
use corvid_time::Tick;
/// The window the bounded runs here keep, in ticks.
///
/// Small enough that a test of a few hundred ticks forgets several times over,
/// and not a divisor of the run lengths below, so a boundary that landed one
/// tick out has somewhere to show.
const WINDOW: u64 = 23;

/// How long the runs are. Nine windows and a bit.
const TICKS: u64 = 213;

/// A run of [`TICKS`] ticks under `retention`.
fn play(retention: Retention) -> Outcome<Tally> {
    App::<Tally, Hands, Painted, Ears>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .retain(retention)
        .for_ticks(TICKS)
        .run()
        .expect("a headless run of a quiet game cannot fail")
}

#[test]
fn a_bounded_run_computes_what_an_unbounded_one_does() {
    let whole = play(Retention::Everything);
    let window = play(Retention::Recent { ticks: WINDOW });

    assert_eq!(window.state, whole.state, "the state the run stopped at");
    assert_eq!(window.session.last(), whole.session.last());
    assert_eq!(window.session.last(), Tick(TICKS));
    assert_eq!(window.exit, whole.exit);

    // Every tick the bounded run still holds, against the run that held them
    // all. This is the assertion a forget that dropped one tick too many or
    // slid the actions along by a row fails.
    let first = window.session.first();
    assert!(
        first > Tick::ZERO,
        "the bounded run has to have forgotten something, or this compares two identical runs"
    );
    let mut at = first;
    while at <= window.session.last() {
        assert_eq!(
            window.session.marks.get(at),
            whole.session.marks.get(at),
            "the mark at {at}",
        );
        at = at.next();
    }

    let mut at = first;
    while at < window.session.last() {
        assert_eq!(
            window.session.log.row(at),
            whole.session.log.row(at),
            "the actions at {at}",
        );
        assert!(window.session.log.is_confirmed(at, PlayerId(0)));
        at = at.next();
    }
}

#[test]
fn a_bounded_run_holds_between_one_window_and_two() {
    let window = play(Retention::Recent { ticks: WINDOW });

    let held = window.session.log.ticks();
    assert!(
        (WINDOW..=WINDOW * 2).contains(&held),
        "a run of {TICKS} ticks with a window of {WINDOW} held {held} rows",
    );
    assert_eq!(
        window.session.marks.len(),
        held + 1,
        "one mark per tick, plus the state it opened at"
    );
    assert_eq!(
        window.session.first(),
        Tick(TICKS - held),
        "the session reopened exactly as far back as it still reaches",
    );

    // The claim the whole change exists for, stated as a comparison rather than
    // as a bound somebody chose: the run held less than it played.
    assert!(held < TICKS);
}

#[test]
fn no_run_length_holds_more_than_twice_the_window() {
    // The bound the documentation states, checked at every length rather than
    // at one. A run's history sawtooths between one window and two, so a single
    // length only ever samples one point of that tooth — and the boundary the
    // loop forgets on is one comparison, which reads just as well one step out.
    // Moving it by a step stretches the tooth by a step, which the assertion
    // above misses at 213 ticks and this one catches at 71.
    //
    // Two windows and change, so that every phase of the sawtooth is sampled
    // several times over and the widest tooth there is falls inside the range.
    let mut widest = 0;
    for ticks in 1..=(3 * WINDOW + 2) {
        let run = App::<Tally, Hands, Painted, Ears>::new()
            .headless()
            .opening(opening::<Tally>(Rules::quiet()))
            .retain(Retention::Recent { ticks: WINDOW })
            .for_ticks(ticks)
            .run()
            .expect("a headless run of a quiet game cannot fail");

        let held = run.session.log.ticks();
        assert_eq!(
            run.session.first(),
            Tick(ticks - held),
            "a run of {ticks} ticks reopened somewhere other than where it reaches",
        );
        assert!(
            held <= WINDOW * 2,
            "a run of {ticks} ticks with a window of {WINDOW} held {held} rows, \
             which is more than twice the window",
        );
        assert!(
            held >= ticks.min(WINDOW),
            "a run of {ticks} ticks held {held} rows, which is less than it promised",
        );
        widest = widest.max(held);
    }

    // And the tooth really does climb to the top of the range, so the bound
    // above is being tested rather than merely never approached. One tick
    // short of twice the window is where it peaks: the run forgets on the tick
    // that completes a window, so the fullest the history is is the tick
    // before that. Pinning the peak rather than a range is what makes a
    // boundary moved by one a failure here as well as above.
    assert_eq!(
        widest,
        WINDOW * 2 - 1,
        "the sawtooth did not peak where it should, so the bound above was never approached",
    );
}

#[test]
fn a_run_shorter_than_its_window_forgets_nothing() {
    let run = App::<Tally, Hands, Painted, Ears>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .retain(Retention::Recent { ticks: WINDOW })
        .for_ticks(WINDOW)
        .run()
        .expect("a headless run of a quiet game cannot fail");

    assert_eq!(run.session.first(), Tick::ZERO);
    assert_eq!(run.session.log.ticks(), WINDOW);
}

#[test]
fn the_default_keeps_a_window_and_a_capture_keeps_everything() {
    // Long enough to pass the default window twice, so that a default which
    // quietly kept everything fails here rather than merely looking generous.
    let ticks = 700;
    let long = App::<Tally, Hands, Painted, Ears>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(ticks)
        .run()
        .expect("a headless run of a quiet game cannot fail");

    let held = long.session.log.ticks();
    assert!(
        held < ticks,
        "the default retention held all {ticks} rows, so nothing is bounded",
    );
    assert!(
        held >= 256,
        "the default window is documented as at least 256 ticks and this run \
         held {held}",
    );

    let pad = Scratchpad::new("retention-capture");
    let recorded = App::<Tally, Hands, Painted, Ears>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .capture(pad.path())
        .for_ticks(ticks)
        .run()
        .expect("a headless run of a quiet game cannot fail");

    assert_eq!(
        recorded.session.log.ticks(),
        ticks,
        "a capture is a request to write the run down, so it keeps the run",
    );
    assert_eq!(recorded.session.first(), Tick::ZERO);
    assert_eq!(recorded.state, long.state, "and records the same run");

    // And a capture that was told otherwise records the window instead, which
    // is the setting winning over the capture rather than the other way round.
    let pad = Scratchpad::new("retention-capture-bounded");
    let bounded = App::<Tally, Hands, Painted, Ears>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .capture(pad.path())
        .retain(Retention::Recent { ticks: WINDOW })
        .for_ticks(ticks)
        .run()
        .expect("a headless run of a quiet game cannot fail");
    assert!(bounded.session.log.ticks() <= WINDOW * 2);
}

#[test]
fn a_bounded_run_can_still_save_replay_and_seek_across_its_window() {
    let run = play(Retention::Recent { ticks: WINDOW });
    let first = run.session.first();

    // Save and load: the session that comes back is the one that went in, and
    // it is a session this build will replay rather than a shape it refuses.
    let bytes = run
        .session
        .save()
        .expect("every part of this session encodes");
    let loaded = Session::<Tally>::load(&bytes, common::schema())
        .expect("a session that forgot its far past is still a session");
    assert_eq!(loaded, run.session);

    // Replay: seeking to where the run stopped reproduces the state it stopped
    // at, which is the whole claim a recording rests on.
    let mut snapshots = Snapshots::new(64 * 1024);
    let (replayed, _) = loaded
        .seek(&mut snapshots, loaded.last())
        .expect("the last tick is always reachable");
    assert_eq!(replayed, run.state);

    // Time-walk: every tick inside the window, and nothing before it.
    let middle = Tick(first.0 + (run.session.last().0 - first.0) / 2);
    for at in [first, middle, run.session.last()] {
        run.session
            .seek(&mut snapshots, at)
            .unwrap_or_else(|why| panic!("tick {at} is inside the window: {why}"));
    }
    assert_eq!(
        run.session
            .seek(&mut snapshots, Tick(first.0 - 1))
            .map(|(state, _)| state),
        Err(Unreachable::Before {
            to: Tick(first.0 - 1),
            first,
        }),
        "and the tick before the window is gone rather than wrong",
    );
}

#[test]
fn a_bounded_run_can_still_be_rolled_back_inside_its_window() {
    // Two seats, because this client confirms its own column every tick and a
    // confirmed entry is refused rather than corrected. The second seat is the
    // one a lockstep transport would be filling in late.
    let mut open = opening::<Tally>(Rules::quiet());
    open.roster.push(seat(1001));
    let mut run = App::<Tally, Hands, Painted, Ears>::new()
        .headless()
        .opening(open)
        .retain(Retention::Recent { ticks: WINDOW })
        .for_ticks(TICKS)
        .run()
        .expect("a headless run of a quiet game cannot fail");

    let mut snapshots = Snapshots::new(64 * 1024);
    let last = run.session.last();
    let (before, _scratch) = run
        .session
        .seek(&mut snapshots, last)
        .expect("the last tick is always reachable");
    assert_eq!(before, run.state);

    // A packet for the other seat, arriving late for a tick inside the window.
    let late = Tick(run.session.first().0 + 2);
    run.session
        .log
        .set(late, PlayerId(1), Action::Bump)
        .expect("the second seat has confirmed nothing");
    snapshots.discard_from(late.next());

    let (after, _) = run
        .session
        .seek(&mut snapshots, last)
        .expect("the last tick is still reachable");
    assert_ne!(
        after, before,
        "a correction inside the window has to reach the state the seek returns",
    );
    assert_ne!(mark(&after), mark(&before));
}

#[test]
fn a_bounded_run_still_publishes_the_mark_for_the_tick_it_is_on() {
    // The mark for the current tick is the one a desync check compares, so a
    // forget that took the trace one tick too far would leave a peer with
    // nothing to say about the tick it is playing. `Progress::mark` is `Option`
    // for a reason that is not this one.
    let (emitter, watch) = channel(
        "retention",
        Progress {
            tick: Tick::ZERO,
            mark: None,
            frames: 0,
            finished: false,
        },
    );
    let run = App::<Tally, Hands, Painted, Ears>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .retain(Retention::Recent { ticks: WINDOW })
        .progress(emitter)
        .for_ticks(TICKS)
        .run()
        .expect("a headless run of a quiet game cannot fail");

    let last = watch.get();
    assert!(last.finished);
    assert_eq!(last.tick, Tick(TICKS));
    assert_eq!(
        last.mark,
        Some(mark(&run.state)),
        "the last progress published names the digest of the state the run \
         stopped at",
    );
}

#[test]
fn a_window_of_nothing_keeps_the_row_it_is_writing() {
    // The degenerate setting, which is legal and is a state clone per tick. It
    // is here because it is the floor the sawtooth is measured from: a session
    // always covers the tick the loop is writing at, so "keep nothing" is one
    // row rather than none.
    let run = App::<Tally, Hands, Painted, Ears>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .retain(Retention::Recent { ticks: 0 })
        .for_ticks(TICKS)
        .run()
        .expect("a headless run of a quiet game cannot fail");

    assert_eq!(run.session.first(), Tick(TICKS - 1));
    assert_eq!(run.session.log.ticks(), 1);
    assert_eq!(run.session.marks.len(), 2);
    assert_eq!(run.session.marks.get(Tick(TICKS)), Some(mark(&run.state)));

    // Every state it passed through is gone, and the one it is on is not.
    let (state, _) = run
        .session
        .seek(&mut Snapshots::new(0), Tick(TICKS))
        .expect("the tick the run stopped on is always reachable");
    assert_eq!(state, run.state);
}
