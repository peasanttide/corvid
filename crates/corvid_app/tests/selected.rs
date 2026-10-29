//! Which frames a capture keeps, and that keeping fewer changes nothing else.
//!
//! `App::capture_frames` decides per displayed frame, before anything is read
//! back, whether that frame gets a row. A headless run has no picture to read
//! back but writes an audio row for every frame it keeps, and the audio rows
//! follow the same selection as the pictures -- so the first three tests here
//! check the decision end to end on any machine. The last one checks the
//! picture path, and only where there is an adapter to draw with.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use std::{fs, path::Path};

use common::{Counting, Rules, Scratchpad, Tally, backstop, opening};
use corvid_app::{App, Frames, Outcome};
use corvid_replay::HashTrace;
use corvid_time::{Tick, Ticks};

/// How far the counted runs below play.
const TICKS: u64 = 12;

/// The names in one subdirectory of a capture, sorted.
fn names(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A headless run of [`TICKS`] ticks captured into `into`, keeping `frames`.
fn counted(into: &Scratchpad, frames: Frames) -> Outcome<Counting> {
    App::<Counting>::new()
        .headless()
        .capture(into.path())
        .capture_frames(frames)
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(TICKS))
        .run()
        .unwrap()
}

/// What a capture that kept only its last frame must hold: one audio row,
/// named `stopped`, no picture, and the trace and session whole.
fn holds_only(root: &Path, run: &Outcome<Counting>, stopped: Tick) {
    assert_eq!(run.session.last(), stopped);
    assert_eq!(names(&root.join("audio")), [stopped.to_string()]);
    assert!(names(&root.join("frames")).is_empty());
    let marks: HashTrace = corvid_wire::decode(&fs::read(root.join("trace")).unwrap()).unwrap();
    assert_eq!(marks, run.session.marks);
    assert!(root.join("session").is_file());
}

#[test]
fn the_last_frame_is_the_one_the_run_stops_on_however_it_stops() {
    // Stopped by a count.
    let scratchpad = Scratchpad::new("last-count");
    let run = counted(&scratchpad, Frames::Last);
    holds_only(scratchpad.path(), &run, Tick(TICKS));

    // Stopped by a predicate, which the loop learns of only after the tick
    // that satisfied it has run.
    let scratchpad = Scratchpad::new("last-until");
    let run = App::<Counting>::new()
        .headless()
        .capture(scratchpad.path())
        .capture_frames(Frames::Last)
        .opening(opening::<Tally>(Rules::quiet()))
        .until(|state: &Tally, _| state.now >= Tick(5))
        .run()
        .unwrap();
    holds_only(scratchpad.path(), &run, Tick(5));

    // Stopped by the game. The tick at seven asks to quit and that tick ran,
    // so the state the run stops on, and the frame it displays, is at eight.
    let scratchpad = Scratchpad::new("last-quit");
    let run = App::<Counting>::new()
        .headless()
        .capture(scratchpad.path())
        .capture_frames(Frames::Last)
        .opening(opening::<Tally>(Rules {
            quit_at: Some(Tick(7)),
            ..Rules::quiet()
        }))
        .run()
        .unwrap();
    holds_only(scratchpad.path(), &run, Tick(8));
}

#[test]
fn listed_ticks_are_the_frames_written_down() {
    // Ninety-nine is never displayed, so it writes nothing and is not an error.
    let scratchpad = Scratchpad::new("at");
    let run = counted(&scratchpad, Frames::At(vec![Tick(3), Tick(7), Tick(99)]));
    assert_eq!(names(&scratchpad.path().join("audio")), ["3", "7"]);
    assert!(names(&scratchpad.path().join("frames")).is_empty());
    assert_eq!(run.session.last(), Tick(TICKS));

    // And a list of nothing keeps nothing, while the capture is still made.
    let scratchpad = Scratchpad::new("none");
    drop(counted(&scratchpad, Frames::At(Vec::new())));
    assert!(names(&scratchpad.path().join("audio")).is_empty());
    assert!(scratchpad.path().join("session").is_file());
}

#[test]
fn every_frame_is_the_default_and_keeping_fewer_computes_the_same_run() {
    let mut every: Vec<String> = (1..=TICKS).map(|tick| tick.to_string()).collect();
    every.sort();

    // Said, and not said: a capture nobody told otherwise keeps every frame.
    let said = Scratchpad::new("every");
    let explicit = counted(&said, Frames::Every);
    assert_eq!(names(&said.path().join("audio")), every);

    let unsaid = Scratchpad::new("default");
    let default = App::<Counting>::new()
        .headless()
        .capture(unsaid.path())
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(TICKS))
        .run()
        .unwrap();
    assert_eq!(names(&unsaid.path().join("audio")), every);

    // The selection is what gets written down and nothing about what is
    // computed, so a run keeping one frame plays the run keeping all of them.
    let one = Scratchpad::new("one");
    let last = counted(&one, Frames::Last);
    assert_eq!(last.session.marks, explicit.session.marks);
    assert_eq!(default.session.marks, explicit.session.marks);
    assert_eq!(last.state, explicit.state);
    assert_eq!(
        fs::read(one.path().join("audio").join(TICKS.to_string())).unwrap(),
        fs::read(said.path().join("audio").join(TICKS.to_string())).unwrap(),
    );
}

/// Whether an error is "this machine cannot draw at all" rather than a capture
/// that went wrong, which has to fail rather than skip.
const fn no_adapter(why: &corvid_app::Error) -> bool {
    matches!(
        why,
        corvid_app::Error::Drew(
            corvid_render::Error::NoAdapter(_) | corvid_render::Error::NoDevice(_)
        )
    )
}

/// An offscreen run of [`TICKS`] ticks captured into `into`, or [`None`] on a
/// machine with no adapter.
#[allow(
    clippy::print_stderr,
    reason = "a skipped test has to say so where a person running the suite will see it, and a tracing event needs a subscriber the harness does not install"
)]
fn drawn(into: &Scratchpad, frames: Frames) -> Option<Outcome<Counting>> {
    let run = App::<Counting>::new()
        .offscreen(corvid_render::Extent::new(64, 64))
        .capture(into.path())
        .capture_frames(frames)
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(TICKS))
        .run();
    match run {
        Ok(outcome) => Some(outcome),
        Err(why) if no_adapter(&why) => {
            eprintln!("skipped: this machine has no adapter to render with ({why})");
            None
        }
        Err(why) => panic!("an offscreen capture failed: {why}"),
    }
}

#[test]
fn an_offscreen_run_reads_back_only_the_frames_it_keeps() {
    backstop::drawing("an offscreen capture keeping some frames", || {
        let last = Scratchpad::new("drawn-last");
        let Some(run) = drawn(&last, Frames::Last) else {
            return;
        };
        assert_eq!(run.session.last(), Tick(TICKS));
        assert_eq!(names(&last.path().join("frames")), [format!("{TICKS}.png")]);
        assert_eq!(names(&last.path().join("audio")), [TICKS.to_string()]);

        let listed = Scratchpad::new("drawn-at");
        drop(drawn(&listed, Frames::At(vec![Tick(3), Tick(7)])));
        assert_eq!(names(&listed.path().join("frames")), ["3.png", "7.png"]);
        assert_eq!(names(&listed.path().join("audio")), ["3", "7"]);
    });
}
