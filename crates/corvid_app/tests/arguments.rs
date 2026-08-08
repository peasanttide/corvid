//! The front door: what a game's `main` does not have to parse, and what
//! applying it to an app does.
//!
//! [`App::launch`](corvid_app::App::launch) reads the process's own arguments,
//! which a test cannot choose — so the seam these are written against is
//! [`Arguments::parse`] and [`App::arguments`](corvid_app::App::arguments), which
//! are the two halves `launch` is made of and are public for exactly this
//! reason.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use std::path::PathBuf;

use common::{Counting, Rules, Scratchpad, Tally, opening};
use corvid_app::{App, Argument, Arguments, Retention};
use corvid_time::Tick;
#[test]
fn the_flags_a_game_gets_for_free() {
    let parsed = Arguments::parse([
        "--headless",
        "--ticks",
        "42",
        "--capture",
        "out/run",
        "--retain",
        "64",
    ])
    .expect("every one of these is an argument this runtime has");

    assert!(parsed.headless);
    assert_eq!(parsed.ticks, Some(42));
    assert_eq!(parsed.capture, Some(PathBuf::from("out/run")));
    assert_eq!(parsed.retain, Some(Retention::Recent { ticks: 64 }));
}

#[test]
fn a_value_may_be_attached_or_may_follow() {
    let attached = Arguments::parse(["--ticks=42", "--capture=out/run", "--retain=all"])
        .expect("the attached spelling is the same argument");
    let following = Arguments::parse(["--ticks", "42", "--capture", "out/run", "--retain", "all"])
        .expect("the following spelling is the same argument");

    assert_eq!(attached, following);
    assert_eq!(attached.retain, Some(Retention::Everything));

    // The split is on the first `=`, so a path with one in it survives.
    let odd = Arguments::parse(["--capture=out/a=b"]).expect("a path may contain an equals sign");
    assert_eq!(odd.capture, Some(PathBuf::from("out/a=b")));
}

#[test]
fn nothing_said_is_nothing_changed() {
    let empty: [&str; 0] = [];
    assert_eq!(
        Arguments::parse(empty).expect("no arguments is a legal command line"),
        Arguments::default(),
    );
}

#[test]
fn every_way_a_command_line_is_refused() {
    assert_eq!(
        Arguments::parse(["--tickss", "3"]),
        Err(Argument::Unknown {
            argument: "--tickss".to_owned(),
        }),
    );
    assert_eq!(
        Arguments::parse(["--ticks"]),
        Err(Argument::Missing { flag: "--ticks" }),
    );
    assert_eq!(
        Arguments::parse(["--ticks", "soon"]),
        Err(Argument::NotANumber {
            flag: "--ticks",
            value: "soon".to_owned(),
        }),
    );
    assert_eq!(
        Arguments::parse(["--retain", "some"]),
        Err(Argument::NotANumber {
            flag: "--retain",
            value: "some".to_owned(),
        }),
    );
    // The case that would otherwise turn a window off by turning it on: a value
    // attached to a flag that takes none.
    assert_eq!(
        Arguments::parse(["--headless=false"]),
        Err(Argument::Unexpected { flag: "--headless" }),
    );
    assert_eq!(Arguments::parse(["--help"]), Err(Argument::Help));
    assert_eq!(Arguments::parse(["-h"]), Err(Argument::Help));

    // Asking for the usage reports the usage, which is the whole of why help is
    // an error here: this crate may not print, so the text has to travel as a
    // value somebody's `main` can put where it wants.
    assert_eq!(Argument::Help.to_string(), Arguments::USAGE);
    assert!(
        Argument::Missing { flag: "--ticks" }
            .to_string()
            .contains(Arguments::USAGE),
        "a refusal says what the runtime does accept",
    );
}

#[test]
fn an_argument_beats_the_builder_and_silence_does_not() {
    let run = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(50)
        .arguments(Arguments::parse(["--ticks=7"]).expect("a count is an argument"))
        .run()
        .expect("a headless run of a quiet game cannot fail");
    assert_eq!(run.session.last(), Tick(7), "the operator's count wins");

    // And the other order, which is the half the documentation claims and an
    // ordinary builder setter does not have: a game's `main` that reads the
    // command line first and then sets its own defaults must still answer to
    // `--ticks`.
    let first = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .arguments(Arguments::parse(["--ticks=7"]).expect("a count is an argument"))
        .for_ticks(50)
        .run()
        .expect("a headless run of a quiet game cannot fail");
    assert_eq!(
        first.session.last(),
        Tick(7),
        "the operator's count wins whichever order the two are written in",
    );

    // Two command lines is one command line, and it is the later one.
    let twice = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .arguments(Arguments::parse(["--ticks=7"]).expect("a count is an argument"))
        .arguments(Arguments::parse(["--ticks=9"]).expect("a count is an argument"))
        .run()
        .expect("a headless run of a quiet game cannot fail");
    assert_eq!(twice.session.last(), Tick(9));

    let empty: [&str; 0] = [];
    let untouched = App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .for_ticks(50)
        .arguments(Arguments::parse(empty).expect("no arguments is a legal command line"))
        .run()
        .expect("a headless run of a quiet game cannot fail");
    assert_eq!(
        untouched.session.last(),
        Tick(50),
        "and an argument nobody gave changes nothing",
    );
}

#[test]
fn the_arguments_do_what_the_builder_calls_do() {
    let pad = Scratchpad::new("arguments-capture");
    let path = pad.path().to_string_lossy().into_owned();
    let long = 700;

    let run = App::<Counting>::new()
        .opening(opening::<Tally>(Rules::quiet()))
        .arguments(
            Arguments::parse(["--headless", &format!("--ticks={long}"), "--capture", &path])
                .expect("every one of these is an argument this runtime has"),
        )
        .run()
        .expect("a headless run of a quiet game cannot fail");

    assert_eq!(run.session.last(), Tick(long));
    assert!(
        pad.path().join("session").is_file(),
        "the capture was written"
    );
    assert_eq!(
        run.session.log.ticks(),
        long,
        "a capture asked for on the command line keeps the run, exactly as one \
         asked for in the builder does",
    );

    // And `--retain` overrides that, which is the one interaction between two
    // of these flags.
    let pad = Scratchpad::new("arguments-capture-bounded");
    let path = pad.path().to_string_lossy().into_owned();
    let bounded = App::<Counting>::new()
        .opening(opening::<Tally>(Rules::quiet()))
        .arguments(
            Arguments::parse([
                "--headless",
                &format!("--ticks={long}"),
                "--capture",
                &path,
                "--retain=32",
            ])
            .expect("every one of these is an argument this runtime has"),
        )
        .run()
        .expect("a headless run of a quiet game cannot fail");
    assert!(bounded.session.log.ticks() <= 64);
}
