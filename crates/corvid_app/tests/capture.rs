//! What a capture holds, and the one claim it exists to support.
//!
//! That claim is the first test here: a run written to a directory, read back
//! from that directory by something that has never seen the process which wrote
//! it, and replayed to the state the run stopped at. Everything else in this
//! file is about the files themselves.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use corvid_behavior::{Extract, Extracting, Time};
use corvid_sound::Auralizer;
use std::{fs, num::NonZeroU32};

use common::{Action, Counting, Ears, Rules, Scratchpad, Tally, opening, schema};
use corvid_app::App;
use corvid_hash::digest;
use corvid_replay::{HashTrace, Load, Schema, Session, Snapshots};
use corvid_sound::{AudioFrame, Hearing};
use corvid_time::{Clock, Tick, TickSpan, Ticks};

/// How far the runs below play.
const TICKS: u64 = 12;

/// The seat this client submits for, which is the only seat these openings
/// have.
const SEAT: corvid_behavior::PlayerId = corvid_behavior::PlayerId(0);

/// A run of [`TICKS`] ticks, written into `where_to`.
fn capture_into(where_to: &Scratchpad) -> corvid_app::Outcome<Counting> {
    App::<Counting>::new()
        .headless()
        .capture(where_to.path())
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(TICKS))
        .run()
        .unwrap()
}

#[test]
fn a_captured_run_replays_to_the_same_state() {
    let scratchpad = Scratchpad::new("replays");
    let run = capture_into(&scratchpad);

    // Nothing of the run is used from here on but the answer it is compared
    // against: the session comes off the disk, through the same `load` a game
    // opening a save file would use.
    let bytes = fs::read(scratchpad.path().join("session")).unwrap();
    let session: Session<Tally> = Session::load(&bytes, schema()).unwrap();
    assert_eq!(session.last(), run.session.last());

    // A budget of zero keeps no snapshots, so this replays every tick from the
    // opening — the longest path through `seek` and the one with nothing
    // cached to hide a mistake.
    let mut snapshots = Snapshots::<Tally>::new(0);
    let (state, _) = session.seek(&mut snapshots, session.last()).unwrap();

    assert_eq!(digest(&state), digest(&run.state));
    assert_eq!(state, run.state);

    // And the trace the replay walks past, tick by tick, so that "the same
    // final state" cannot be bought by a session that took a different route
    // to it.
    assert_eq!(session.marks, run.session.marks);

    // The digests above cannot see a loop that never logged an action, and it
    // is worth being exact about why rather than leaving it to be discovered.
    // The loop ticks *from* the log — it writes this client's action into the
    // row and then reads the whole row back — so a run that logged nothing is a
    // run that played a game in which this client did nothing, and it replays to
    // itself perfectly. What catches that is the log's own confirmation bit,
    // which says somebody wrote the entry rather than that the entry holds the
    // default.
    let mut tick = session.first();
    while tick < session.last() {
        assert!(
            session.log.is_confirmed(tick, SEAT),
            "nothing was recorded for seat {} at tick {tick}",
            SEAT.0,
        );
        tick = tick.next();
    }

    // And that what was recorded is not the default everywhere, so the run
    // being replayed is a run in which something happened.
    let mut acted = 0;
    let mut tick = session.first();
    while tick < session.last() {
        acted += usize::from(session.log.get(tick, SEAT) != Some(&Action::default()));
        tick = tick.next();
    }
    assert!(acted > 0, "every action in the session is the default");
}

#[test]
fn a_capture_is_refused_by_a_build_that_describes_its_types_differently() {
    let scratchpad = Scratchpad::new("schema");
    drop(capture_into(&scratchpad));

    let bytes = fs::read(scratchpad.path().join("session")).unwrap();
    let elsewhere = Schema::new("tally").field("Tally.count", "i128").digest();
    match Session::<Tally>::load(&bytes, elsewhere) {
        Err(Load::Schema { recorded, running }) => {
            assert_eq!(recorded, schema());
            assert_eq!(running, elsewhere);
        }
        other => panic!("a capture from another build was accepted: {other:?}"),
    }
}

#[test]
fn a_capture_holds_the_four_things_it_says_it_does() {
    let scratchpad = Scratchpad::new("layout");
    let run = capture_into(&scratchpad);
    let root = scratchpad.path();

    // The trace, whole, as its own file rather than only inside the session.
    let marks: HashTrace = corvid_wire::decode(&fs::read(root.join("trace")).unwrap()).unwrap();
    assert_eq!(marks, run.session.marks);

    // One frame per displayed frame, named for the tick the frame's `current`
    // state is at. The loop displays once per iteration and this clock owes it
    // one tick per iteration, so these are ticks one through twelve and there
    // is none at the opening tick.
    //
    // That last part is a property of *this clock* rather than of the loop:
    // `a_clock_slower_than_the_tick_rate_displays_at_the_opening_tick` is the
    // same assertion under a quarter-period clock, where the answer is the
    // other one.
    let names: Vec<String> = fs::read_dir(root.join("audio"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names.len(),
        usize::try_from(TICKS).unwrap(),
        "audio holds {names:?}",
    );
    assert!(
        !root.join("audio").join("0").exists(),
        "audio has a frame at the opening tick"
    );
    assert!(root.join("audio").join(TICKS.to_string()).exists());

    // And `frames/` is there and empty, which is the whole of what "a headless
    // run writes no picture" means as a fact rather than as a sentence: a
    // picture is read back off an offscreen texture and this run has no
    // adapter, so the directory exists — a directory that is sometimes absent
    // is a second thing for a comparison to be confused by — and nothing is in
    // it. `tests/windowless.rs` is where a run that does have a device writes
    // into it.
    assert_eq!(fs::read_dir(root.join("frames")).unwrap().count(), 0);

    // Nothing else. A capture that had quietly grown a fifth kind of file would
    // be a capture whose documentation is out of date, and the documentation is
    // most of what a capture format is.
    let mut entries: Vec<String> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    assert_eq!(entries, ["audio", "frames", "session", "trace"]);
}

#[test]
fn a_captured_audio_frame_is_the_one_the_extractor_produced_at_that_tick() {
    // The one per-frame file a headless capture still holds, and the whole of
    // what a run with no device can be compared on frame by frame. It is not a
    // formality: counting the files in `audio/` and stopping cannot see the
    // runtime's per-frame `AudioFrame::clear`. Without it the frame handed to
    // `hear` keeps every
    // source and every cue from every frame before it, the file count is
    // exactly the same, and the capture quietly grows a frame's worth of
    // sources per tick.
    let scratchpad = Scratchpad::new("audio");
    let run = capture_into(&scratchpad);
    let session = run.session;

    let recorded = |at: Tick| -> AudioFrame {
        let bytes = fs::read(scratchpad.path().join("audio").join(at.to_string())).unwrap();
        corvid_wire::decode(&bytes).unwrap()
    };

    // The fixture's `hear` writes one listener, between one and five sources,
    // and a cue on a frame whose tally moved. So the count never exceeds five,
    // and a frame that accumulated would be past that by tick six and at fifty
    // by the last.
    for tick in [1_u64, 4, TICKS] {
        let frame = recorded(Tick(tick));
        assert!(
            (1..=5).contains(&frame.sources.len()),
            "tick {tick}: {frame:?}",
        );
        assert!(frame.cues.len() <= 1, "tick {tick}: {frame:?}");
    }

    // And the whole frame, against what the game's own `hear` would have
    // emitted for that tick, rebuilt from the session alone — the same
    // comparison the frames get, so a capture that wrote the right frame
    // under the wrong name fails here.
    let extracted = |at: Tick| -> AudioFrame {
        let (previous, _) = session.seek(&mut Snapshots::new(0), at.prev()).unwrap();
        let (current, _) = session.seek(&mut Snapshots::new(0), at).unwrap();
        let mut frame = AudioFrame::new();
        // The ear is extracted into rather than handed a pair: it keeps its own
        // previous and current, so feeding it the two states in order is what
        // reproduces what the loop did.
        let mut ears = <Ears as Auralizer<Tally>>::new(());
        ears.extract(Extracting {
            state: &previous,
            level: &session.opening.content,
            time: Time::default(),
        });
        ears.extract(Extracting {
            state: &current,
            level: &session.opening.content,
            time: Time::default(),
        });
        ears.hear(Hearing {
            out: &mut frame,
            camera: &corvid_camera::Camera::default(),
            time: Time::default(),
        });
        frame
    };

    for tick in [1_u64, 4, TICKS] {
        assert_eq!(recorded(Tick(tick)), extracted(Tick(tick)), "tick {tick}");
    }

    // And two of the recorded frames really are different, so the comparison
    // above is not three copies of one answer: the tally moves on one tick in
    // three, so one of these carries a cue and the other does not.
    assert_ne!(recorded(Tick(1)).cues.len(), recorded(Tick(2)).cues.len());

    // The alpha half of this test is gone, and it is worth saying why rather
    // than leaving a shorter test behind. It used to build two frames at
    // `Factor16::ZERO` and `ONE` and assert they differed, because `hear` was
    // handed the pair and the alpha between them.
    //
    // An ear is extracted into now. It keeps its own previous and current and
    // is never told an alpha — interpolation is the renderer's, and it happens
    // in a shader. So there is no alpha here to be right or wrong about, and
    // the comparison above is the whole of what this test can still claim.
}

#[test]
fn a_clock_slower_than_the_tick_rate_displays_at_the_opening_tick() {
    // "There is never a frame at the opening tick, because the first frame is
    // displayed after the first tick" is true of the default clock and false in
    // general, and this is the run that says so: a quarter-period clock owes no
    // tick on its first three readings
    // and the loop displays on every one of them, because a display that waited
    // for a tick would stutter whenever the simulation is not due.
    let rate = TickSpan::from_hz(NonZeroU32::new(20).unwrap());
    let scratchpad = Scratchpad::new("slow");
    let run = App::<Counting>::new()
        .headless()
        .rate(rate)
        .clock(Clock::stepping(rate.period() / 4))
        .capture(scratchpad.path())
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(2))
        .run()
        .unwrap();

    assert_eq!(run.session.last(), Tick(2));
    assert!(
        scratchpad.path().join("audio").join("0").is_file(),
        "audio has no frame at the opening tick",
    );

    // The neighbour: the same run under the default clock, where a reading is a
    // whole period, has no frame there. So the file above is about the clock
    // rather than about a loop that always displays before it ticks.
    let quick = Scratchpad::new("quick");
    drop(
        App::<Counting>::new()
            .headless()
            .rate(rate)
            .capture(quick.path())
            .opening(opening::<Tally>(Rules::quiet()))
            .for_ticks(Ticks(2))
            .run()
            .unwrap(),
    );
    assert!(
        !quick.path().join("audio").join("0").exists(),
        "audio has a frame at the opening tick under the default clock",
    );
}

#[test]
fn capturing_does_not_change_what_a_run_computes() {
    // A capture is an observation. If writing it moved the trace, every golden
    // in the workspace would be about the observed run rather than about the
    // game.
    let scratchpad = Scratchpad::new("observed");
    let watched = capture_into(&scratchpad);
    let unwatched = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(TICKS))
        .run()
        .unwrap();

    assert_eq!(watched.session.marks, unwatched.session.marks);
    assert_eq!(watched.state, unwatched.state);
}

#[test]
fn a_capture_directory_is_created_where_it_was_asked_for() {
    let scratchpad = Scratchpad::new("mkdir");
    let nested = scratchpad.path().join("deep").join("under");
    assert!(!nested.exists());

    let run = App::<Counting>::new()
        .headless()
        .capture(&nested)
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(Ticks(2))
        .run()
        .unwrap();

    assert_eq!(run.session.last(), Tick(2));
    assert!(nested.join("session").is_file());
    assert!(nested.join("audio").is_dir());
}
