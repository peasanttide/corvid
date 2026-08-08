//! Save and load, which every Corvid game has without asking for it.
//!
//! The game below implements nothing for any of this. Its `Tally` is `Data`,
//! and that is the whole requirement: the runtime writes the session and the
//! state, and reading one back is `Session::seek` — the same call rollback and
//! time-walk are.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use std::{
    fs,
    path::{Path, PathBuf},
};

use common::{Counting, FAREWELL, Rules, SLOT, Scratchpad, Tally, opening, schema};
use corvid_app::Command;
use corvid_app::{Answer, App, Error, NotASave};
use corvid_behavior::SaveSlot;
use corvid_hash::digest;
use corvid_replay::{Session, Snapshots};
use corvid_time::{Tick, Ticks};
use serde::{Deserialize, Serialize};

/// How far the runs below play when nothing stops them earlier.
const TICKS: u64 = 12;

/// The tick the game saves on.
const SAVED_AT: Tick = Tick(4);

/// The shape of a slot's file: the session, encoded, and the state at its last
/// tick, encoded beside it.
///
/// Spelled out here rather than reached for, because the runtime's own
/// declaration is private — and that is what makes the test below possible at
/// all. Nothing a game can do produces a save whose two halves disagree; what
/// produces one is a build whose arithmetic moved while its schema did not, and
/// there is no way to stand in for such a build except to write the bytes it
/// would have written.
#[derive(Serialize, Deserialize)]
struct Written {
    /// The session, as `Session::save` wrote it.
    session: Vec<u8>,
    /// The state at the session's last tick.
    state: Vec<u8>,
}

/// Where the slots are, under a game's own directory.
///
/// `--state DIR` names the directory a game keeps everything in, and the slots
/// are one of the three things under it — so a test that looks at a slot file
/// joins the same leaf the runtime does.
fn slots(state: &Path) -> PathBuf {
    state.join("saves")
}

/// Makes every write to [`SLOT`] under `state` fail, and leaves whatever is
/// already in the slot alone.
///
/// The bytes of a save go to a file beside the slot and are renamed over it, so
/// putting a directory where that file goes is a write that cannot succeed and
/// cannot touch the slot on its way to not succeeding — which is exactly the
/// failure the atomic write exists for, arranged without a full disk or a
/// process killed halfway through.
fn make_saving_fail(state: &Path) {
    fs::create_dir_all(slots(state).join(format!("{}.corvid.new", SLOT.0))).unwrap();
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
    // tick has already run — so the session in the slot ends one tick past the
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

    // The same tick, the same state, and the same digest — which is the whole
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
    // stopped — which is what makes a recording something to look at rather
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
    // else now — what it writes is the session and the state, which the runtime
    // holds — so there are no bytes here to assert on.
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

#[test]
fn a_save_that_cannot_be_written_leaves_the_one_it_was_replacing_readable() {
    let scratchpad = Scratchpad::new("torn");
    let state = scratchpad.path();

    // The run somebody had going, saved at tick four. This is the hour that a
    // truncate-then-write would put at risk.
    let recorded = play(
        state,
        Rules {
            save_at: Some(SAVED_AT),
            ..Rules::quiet()
        },
        TICKS,
    );

    make_saving_fail(state);

    // A second run of a game whose tally moves by a different amount, so the
    // save it wants to write is not the save that is there and the two are
    // distinguishable. It asks to save into the same slot and cannot.
    let refused = play(
        state,
        Rules {
            save_at: Some(SAVED_AT),
            step: 11,
            ..Rules::quiet()
        },
        TICKS,
    );
    assert_eq!(refused.requests.failed().count(), 1);
    assert_eq!(refused.session.last(), Tick(TICKS));

    // And the slot still holds the first run's save, whole: a third run opens on
    // it and arrives exactly where the first one did, which it could not do from
    // a prefix and would not do from the second run's save.
    let opened_at = SAVED_AT.next();
    let resumed = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .state(state)
        .load(SLOT)
        .for_ticks(Ticks(TICKS - opened_at.0))
        .run()
        .unwrap();
    assert_eq!(resumed.session.last(), Tick(TICKS));
    assert_eq!(resumed.state, recorded.state);
    assert_eq!(digest(&resumed.state), digest(&recorded.state));
}

#[test]
fn a_run_whose_save_fails_keeps_its_capture_and_says_the_save_failed() {
    // The three things a failing save may not cost the run: the commands after
    // it in the same tick, the capture, and any word of what went wrong.
    let scratchpad = Scratchpad::new("unsaved");
    let state = scratchpad.path().join("slots");
    let capture = scratchpad.path().join("capture");
    make_saving_fail(&state);

    // One tick both asks to save and asks to quit, in that order — which is the
    // arrangement that would lose the quit if the failing save aborted the
    // drain.
    let run = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules {
            save_at: Some(SAVED_AT),
            quit_at: Some(SAVED_AT),
            ..Rules::quiet()
        }))
        .state(&state)
        .capture(&capture)
        .for_ticks(Ticks(TICKS))
        .run()
        .unwrap();

    // It said so, and it said so about the tick that asked.
    let failed: Vec<_> = run.requests.failed().collect();
    assert_eq!(failed.len(), 1, "{:?}", run.requests);
    assert_eq!(failed[0].tick, SAVED_AT);
    // The save that failed is the only save, and it is recorded as failed
    // rather than dropped. There are no bytes to check for absence any more:
    // a save carries a slot and the runtime writes the session and the state,
    // so what a failure leaves behind is the record and nothing else.
    assert!(
        failed
            .iter()
            .all(|request| matches!(request.command, Command::Save(_))),
    );

    // The quit behind it in the same list was still absorbed, with its status.
    assert_eq!(run.exit, FAREWELL);

    // And the capture is a capture rather than a directory of frames: the
    // session and the trace are there, and replaying the one lands on the state
    // the run stopped at.
    let bytes = fs::read(capture.join("session")).unwrap();
    let session: Session<Tally> = Session::load(&bytes, schema()).unwrap();
    assert_eq!(session.last(), run.session.last());
    let mut snapshots = Snapshots::<Tally>::new(0);
    let (state, _) = session.seek(&mut snapshots, session.last()).unwrap();
    assert_eq!(digest(&state), digest(&run.state));
    assert!(capture.join("trace").is_file());
}

#[test]
fn a_save_whose_recorded_state_is_not_what_its_own_log_replays_to_is_refused() {
    // The refusal a schema digest cannot make. Both builds describe their types
    // identically — so `Session::load` is happy — and one of them computes
    // something else out of them. Reading the slot replays its log and compares
    // the answer against the state written beside it, and this is the arm where
    // the two disagree.
    let scratchpad = Scratchpad::new("diverged");
    let state = scratchpad.path();
    play(
        state,
        Rules {
            save_at: Some(SAVED_AT),
            ..Rules::quiet()
        },
        TICKS,
    );

    // Move one column of the recorded state and put the file back. The session
    // beside it is untouched, so its log still replays to what it always did.
    let path = slots(state).join(format!("{}.corvid", SLOT.0));
    let mut written: Written = corvid_wire::decode(&fs::read(&path).unwrap()).unwrap();
    let mut recorded: Tally = corvid_wire::decode(&written.state).unwrap();
    recorded.count += 1;
    written.state = corvid_wire::encode(&recorded).unwrap();
    fs::write(&path, corvid_wire::encode(&written).unwrap()).unwrap();

    let why = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .state(state)
        .load(SLOT)
        .for_ticks(Ticks(0))
        .run()
        .unwrap_err();
    let Error::Saved {
        why: NotASave::Diverged { recorded, replayed },
        ..
    } = why
    else {
        panic!("a save that does not replay to its own recorded state was accepted: {why}");
    };
    assert_ne!(recorded, replayed);
}

#[test]
fn opening_a_slot_nothing_has_written_is_refused_rather_than_started_over() {
    // A run that was asked to resume and quietly started a new game is a run
    // that has lost somebody's save, so the empty slot is an error at start-up.
    let scratchpad = Scratchpad::new("empty");
    let why = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .state(scratchpad.path())
        .load(SaveSlot(9))
        .for_ticks(Ticks(1))
        .run()
        .unwrap_err();
    assert!(
        matches!(why, Error::Empty { slot } if slot == SaveSlot(9)),
        "{why}",
    );
}

#[test]
fn a_tick_that_reads_a_slot_nothing_wrote_is_told_so_and_the_run_carries_on() {
    let scratchpad = Scratchpad::new("reads");
    let run = play(
        scratchpad.path(),
        Rules {
            read_at: Some(Tick(3)),
            ..Rules::quiet()
        },
        TICKS,
    );

    // Empty rather than unhandled: the runtime looked, and there was nothing
    // there. The run plays every tick it was asked for either way.
    let read = run
        .requests
        .iter()
        .find(|request| request.tick == Tick(3))
        .expect("the tick at three asked for something");
    assert_eq!(read.answer, Answer::Empty);
    assert_eq!(run.session.last(), Tick(TICKS));
    assert!(run.requests.unhandled().next().is_none());
}

#[test]
fn a_read_finds_what_an_earlier_run_wrote_and_leaves_this_run_where_it_was() {
    let scratchpad = Scratchpad::new("rereads");
    let state = scratchpad.path();

    // One run writes the slot. A second, playing its own game, asks whether
    // there is anything in it.
    play(
        state,
        Rules {
            save_at: Some(SAVED_AT),
            ..Rules::quiet()
        },
        TICKS,
    );
    let run = play(
        state,
        Rules {
            read_at: Some(Tick(2)),
            ..Rules::quiet()
        },
        TICKS,
    );

    let read = run
        .requests
        .iter()
        .find(|request| request.tick == Tick(2))
        .expect("the tick at two asked for something");
    assert_eq!(read.answer, Answer::Done);

    // And the run it interrupted is not interrupted. Putting a whole session in
    // front of a simulation that is already playing another is a barrier across
    // every peer rather than a file operation, and `--load` is where a slot is
    // opened today — at start-up, where there is no session to interrupt.
    assert_eq!(run.session.last(), Tick(TICKS));
    assert_eq!(run.session.first(), Tick::ZERO);
}
