//! What an argument does once a run acts on it.
//!
//! The seam against `arguments.rs` is the run: that file is the parser and
//! stops at the parsed value, and these play a game with the value applied.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Counting, Rules, Scratchpad, Tally, opening};
use corvid_app::{App, Argument, Arguments};
use corvid_time::Tick;

#[test]
fn the_arguments_do_what_the_builder_calls_do() {
    let pad = Scratchpad::new("arguments-record");
    let file = pad.path().join("session");
    let named = file.to_string_lossy().into_owned();
    let long = 700;

    let run = App::<Counting>::new()
        .opening(opening::<Tally>(Rules::quiet()))
        .arguments(
            Arguments::parse(["--headless", &format!("--ticks={long}"), "--record", &named])
                .expect("every one of these is an argument this runtime has"),
        )
        .run()
        .expect("a headless run of a quiet game cannot fail");

    assert_eq!(run.session.last(), Tick(long));
    assert!(file.is_file(), "the session was written");
    assert_eq!(
        run.session.log.ticks(),
        long,
        "a recording asked for on the command line keeps the run, exactly as one \
         asked for in the builder does",
    );

    // And the file is one `--demo` opens: the second run carries the first on
    // from where it stopped.
    let carried = App::<Counting>::new()
        .opening(opening::<Tally>(Rules::quiet()))
        .arguments(
            Arguments::parse(["--headless", "--ticks=10", "--demo", &named])
                .expect("a recording is a way of opening"),
        )
        .run()
        .expect("a run carrying a recording on");
    assert_eq!(carried.session.last(), Tick(long + 10));
}

/// `--level` opens on the level it names -- the reference *and* the content,
/// because the reference is hashed into nothing and a flag that moved only it
/// would rename the level a run is already on rather than choose one.
#[test]
fn a_level_this_game_has_is_what_the_run_opens_on() {
    let run = App::<Counting>::new()
        .opening(opening::<Tally>(Rules::quiet()))
        .arguments(
            Arguments::parse(["--headless", "--ticks=2", "--level", "meadow"])
                .expect("a level is named by whatever the game calls it"),
        )
        .run()
        .expect("this game builds that level from its name");

    assert_eq!(run.session.opening.level, "meadow");
    assert_eq!(
        run.session.opening.content.name, "meadow",
        "the content moved with the reference, so this run played the level it named",
    );
}

/// A `--level` the game's own loader refuses is refused, rather than opened on
/// whatever content the opening happened to carry.
#[test]
fn a_level_this_game_cannot_open_on_is_refused() {
    let why = App::<Counting>::new()
        .opening(opening::<Tally>(Rules::quiet()))
        .arguments(
            Arguments::parse(["--level", common::ELSEWHERE]).expect("the parser holds a string"),
        )
        .run()
        .expect_err("this game keeps that level somewhere a name cannot reach");
    let corvid_app::Error::Argument(why) = why else {
        panic!("a level that will not load is an argument this run could not act on: {why:?}");
    };
    assert!(matches!(why, Argument::UnreadableLevel { .. }), "{why:?}");
    let said = why.to_string();
    assert!(said.contains("elsewhere"), "{said}");
    assert!(
        said.contains("there is nothing to read it from"),
        "the game's own loader is what says why: {said}",
    );
}

/// `--state DIR` is the one directory a game keeps anything in, so a run told
/// where it is writes its save under it and nowhere else.
#[test]
fn the_state_directory_is_where_a_save_lands() {
    let pad = Scratchpad::new("arguments-state");
    let named = pad.path().to_string_lossy().into_owned();

    let run = App::<Counting>::new()
        .opening(opening::<Tally>(Rules {
            save_at: Some(Tick(2)),
            ..Rules::quiet()
        }))
        .arguments(
            Arguments::parse(["--headless", "--ticks=6", "--state", &named])
                .expect("a directory is an argument this runtime has"),
        )
        .run()
        .expect("a headless run of a quiet game cannot fail");

    assert_eq!(run.session.last(), Tick(6));
    assert!(
        pad.path()
            .join("saves")
            .join(format!("{}.corvid", common::SLOT.0))
            .is_file(),
        "the slot is under the state directory's own saves/",
    );
}
