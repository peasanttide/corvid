//! Save and load, which every Corvid game has without asking for it.
//!
//! The game below implements nothing for any of this. Its `Tally` is `Data`,
//! and that is the whole requirement: the runtime writes the session and the
//! state, and reading one back is `Session::seek` -- the same call rollback and
//! time-walk are.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use std::path::{Path, PathBuf};

use common::{Counting, Rules, SLOT, Scratchpad, Tally, opening};
use corvid_app::{Answer, App};
use corvid_hash::digest;
use corvid_time::{Tick, Ticks};

/// How far the runs below play when nothing stops them earlier.
const TICKS: u64 = 12;

/// The tick the game saves on.
const SAVED_AT: Tick = Tick(4);

/// Where the slots are, under a game's own directory.
///
/// `--state DIR` names the directory a game keeps everything in, and the slots
/// are one of the three things under it -- so a test that looks at a slot file
/// joins the same leaf the runtime does.
fn slots(state: &Path) -> PathBuf {
    state.join("saves")
}

/// A run of the game with the rules given, keeping its files under `state`.
fn play(state: &Path, rules: Rules, ticks: u64) -> corvid_app::Outcome<Counting> {
    App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(rules))
        .state(state)
        .for_ticks(Ticks(ticks))
        .run()
        .unwrap()
}

#[test]
fn a_run_that_loads_what_another_run_saved_reaches_the_same_state() {
    let scratchpad = Scratchpad::new("saves");
    let state = scratchpad.path();

    // The first run saves at tick four and plays on to twelve. This is the run
    // a player had going.
    let recorded = play(
        state,
        Rules {
            save_at: Some(SAVED_AT),
            ..Rules::quiet()
        },
        TICKS,
    );
    assert_eq!(recorded.session.last(), Tick(TICKS));

    // The second is a fresh process as far as the runtime is concerned: a new
    // `App`, a new session, nothing carried over but the directory.
    //
    // A save is written when the asking tick's commands are drained, and that
    // tick has already run -- so the session in the slot ends one tick past the
    // one that asked, and this run opens there. `--ticks` counts from where a
    // run opened, so what is left is the rest of the twelve.
    let opened_at = SAVED_AT.next();
    let resumed = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .state(state)
        .load(SLOT)
        .for_ticks(Ticks(TICKS - opened_at.0))
        .run()
        .unwrap();

    // The same tick, the same state, and the same digest -- which is the whole
    // claim: a save is a session, and a session replays to the game it was.
    assert_eq!(resumed.session.last(), Tick(TICKS));
    assert_eq!(resumed.state, recorded.state);
    assert_eq!(digest(&resumed.state), digest(&recorded.state));

    // And the history came back with it rather than being started over: the
    // resumed session covers the ticks before the save as well as the ones
    // after, and marks them the way the first run did.
    assert_eq!(resumed.session.first(), recorded.session.first());
    for tick in 0..=TICKS {
        let at = Tick(tick);
        assert_eq!(
            resumed.session.marks.get(at),
            recorded.session.marks.get(at),
            "the two runs marked tick {at} differently",
        );
    }
}

#[test]
fn a_run_can_open_on_the_session_a_capture_recorded() {
    // `App::replay` is the other way into a session that already happened, and
    // the file is the `session` a capture directory holds. A run that opens on
    // it and plays no further ticks is standing exactly where the recorded run
    // stopped -- which is what makes a recording something to look at rather
    // than something to take somebody's word for.
    let scratchpad = Scratchpad::new("replay");
    let capture = scratchpad.path().join("capture");
    let recorded = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .capture(&capture)
        .for_ticks(Ticks(TICKS))
        .run()
        .unwrap();

    let resumed = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .replay(capture.join("session"))
        .for_ticks(Ticks(0))
        .run()
        .unwrap();

    assert_eq!(resumed.session.last(), recorded.session.last());
    assert_eq!(resumed.state, recorded.state);
    assert_eq!(digest(&resumed.state), digest(&recorded.state));
}

/// The other way of writing that file, and the claim that it is the same file:
/// `--record` writes what `--demo` opens, without a capture directory around
/// it.
#[test]
fn a_recorded_session_is_one_a_demo_opens() {
    let pad = Scratchpad::new("recorded");
    let file = pad.path().join("session");

    let first = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(20))
        .record(&file)
        .run()
        .expect("a recorded run");

    let second = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .replay(&file)
        .for_ticks(Ticks(10))
        .run()
        .expect("a run carrying it on");

    assert_eq!(second.session.first(), first.session.first());
    assert_eq!(second.session.last().0, first.session.last().0 + 10);
    // The trace joins up: the tick the first run stopped at has the same mark
    // in both sessions.
    assert_eq!(
        second.session.marks.get(first.session.last()),
        first.session.marks.get(first.session.last())
    );
}

#[test]
fn a_save_lands_in_the_slot_it_named_and_nowhere_else() {
    let scratchpad = Scratchpad::new("slots");
    let directory = scratchpad.path();

    let run = play(
        directory,
        Rules {
            save_at: Some(SAVED_AT),
            ..Rules::quiet()
        },
        TICKS,
    );

    // The request is recorded against the tick that made it, and the bytes the
    // game asked to put there are what `Requests::saved` reports.
    let done = run
        .requests
        .iter()
        .filter(|request| request.answer == Answer::Done)
        .filter(|request| request.tick == SAVED_AT)
        .count();
    assert_eq!(done, 1);
    // The request was made and acted on. A save carries a slot and nothing
    // else now -- what it writes is the session and the state, which the runtime
    // holds -- so there are no bytes here to assert on.
    assert!(
        run.requests
            .iter()
            .any(|request| request.answer == corvid_app::Answer::Done),
    );

    // One file, named for the slot. A slot nothing wrote is not there, which is
    // what `Answer::Empty` and `Error::Empty` are about.
    assert!(
        slots(directory)
            .join(format!("{}.corvid", SLOT.0))
            .is_file()
    );
    assert!(!slots(directory).join("0.corvid").exists());
}
