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
    clippy::panic,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Counting, Rules, Scratchpad, Tally, opening};
use corvid_app::{App, Argument, Arguments, Load};
use corvid_behavior::{PlayerId, SaveSlot};
use corvid_time::{Tick, Ticks};
use std::path::Path;

#[test]
fn nothing_at_all_is_every_default() {
    let parsed = Arguments::parse(Vec::<String>::new()).expect("no arguments");
    assert!(!parsed.headless);
    assert!(!parsed.spectator);
    assert_eq!(parsed.num_bots, 0);
    assert_eq!(parsed.ticks, None);
    assert_eq!(parsed.load, None);
    assert_eq!(parsed.record, None);
    assert_eq!(parsed.state, None);
    assert_eq!(parsed.seat, PlayerId(0));
    assert_eq!(parsed.listen, None);
    assert_eq!(parsed.connect, None);
}

/// Every flag but one, and the one is below: `--bots` cannot share a command
/// line with `--connect`, which is the refusal the test after next is about, so
/// a line naming every flag at once is a line this runtime does not have.
#[test]
fn every_flag_is_read() {
    let parsed = Arguments::parse([
        "--headless",
        "--spectator",
        "--ticks",
        "90",
        "--record",
        "out/session",
        "--state",
        "here/",
        "--seat",
        "1",
        "--listen",
        "9000",
        "--connect",
        "host:9001",
    ])
    .expect("every flag");

    assert!(parsed.headless);
    assert!(parsed.spectator);
    assert_eq!(parsed.num_bots, 0);
    assert_eq!(parsed.ticks, Some(Ticks(90)));
    assert_eq!(parsed.record.as_deref(), Some(Path::new("out/session")));
    assert_eq!(parsed.state.as_deref(), Some(Path::new("here/")));
    assert_eq!(parsed.seat, PlayerId(1));
    assert_eq!(parsed.listen, Some(9000));
    assert_eq!(parsed.connect.as_deref(), Some("host:9001"));

    let botted = Arguments::parse(["--bots", "3"]).expect("the flag the line above cannot have");
    assert_eq!(botted.num_bots, 3);
}

#[test]
fn the_attached_spelling_is_the_same_argument() {
    let parsed = Arguments::parse(["--ticks=90", "--bots=2"]).expect("attached values");
    assert_eq!(parsed.ticks, Some(Ticks(90)));
    assert_eq!(parsed.num_bots, 2);

    // The split is on the *first* `=`, so a path with one in it survives being
    // attached rather than being cut in half.
    let odd = Arguments::parse(["--record=out/a=b"]).expect("a path may contain an equals sign");
    assert_eq!(odd.record.as_deref(), Some(Path::new("out/a=b")));
}

#[test]
fn each_way_of_opening_lands_in_the_one_field() {
    assert_eq!(
        Arguments::parse(["--load", "3"]).expect("a slot").load,
        Some(Load::Save(SaveSlot(3)))
    );
    assert_eq!(
        Arguments::parse(["--demo", "run/session"])
            .expect("a recording")
            .load,
        Some(Load::Demo("run/session".into()))
    );
    assert_eq!(
        Arguments::parse(["--level", "\"Court\""])
            .expect("a level")
            .load,
        Some(Load::Level("\"Court\"".to_owned()))
    );
}

#[test]
fn two_ways_of_opening_is_a_refusal_naming_both() {
    let why = Arguments::parse(["--load", "3", "--demo", "run/session"])
        .expect_err("two ways of opening");
    assert_eq!(
        why,
        Argument::Conflicting {
            flags: ["--load", "--demo"]
        }
    );
}

#[test]
fn bots_and_a_peer_is_a_refusal() {
    let why =
        Arguments::parse(["--bots", "1", "--connect", "host:9001"]).expect_err("bots and a peer");
    assert_eq!(
        why,
        Argument::Conflicting {
            flags: ["--bots", "--connect"]
        }
    );
}

#[test]
fn a_flag_that_takes_a_value_and_is_given_none_is_missing() {
    assert_eq!(
        Arguments::parse(["--ticks"]).expect_err("no value"),
        Argument::Missing { flag: "--ticks" }
    );
}

#[test]
fn a_value_on_a_flag_that_takes_none_is_refused() {
    assert_eq!(
        Arguments::parse(["--headless=false"]).expect_err("a value"),
        Argument::Unexpected { flag: "--headless" }
    );
}

#[test]
fn a_count_that_is_not_a_number_is_refused() {
    assert!(matches!(
        Arguments::parse(["--bots", "many"]).expect_err("not a number"),
        Argument::NotANumber { flag: "--bots", .. }
    ));
}

#[test]
fn asking_for_the_usage_is_reported_rather_than_printed() {
    assert_eq!(Arguments::parse(["-h"]).expect_err("help"), Argument::Help);
    assert_eq!(
        Arguments::parse(["--help"]).expect_err("help"),
        Argument::Help
    );
}

/// The two refusals name themselves in the order the operator wrote them, which
/// is what makes a message about two flags a message about *this* command line.
#[test]
fn a_refusal_names_the_two_flags_in_the_order_they_were_given() {
    assert_eq!(
        Arguments::parse(["--demo", "run/session", "--load", "3"]).expect_err("two openings"),
        Argument::Conflicting {
            flags: ["--demo", "--load"]
        }
    );
    assert_eq!(
        Arguments::parse(["--connect", "host:9001", "--bots", "1"]).expect_err("a peer and bots"),
        Argument::Conflicting {
            flags: ["--connect", "--bots"]
        }
    );
}

/// A flag not this runtime's, and a level that is not JSON, are both refused
/// with the usage attached — because what an operator who typed one of those
/// wants next is the list of what there is.
#[test]
fn every_refusal_says_what_the_runtime_does_accept() {
    assert_eq!(
        Arguments::parse(["--tickss", "3"]).expect_err("no such flag"),
        Argument::Unknown {
            argument: "--tickss".to_owned(),
        },
    );
    assert_eq!(Argument::Help.to_string(), Arguments::USAGE);
    for why in [
        Argument::Missing { flag: "--ticks" },
        Argument::Conflicting {
            flags: ["--load", "--demo"],
        },
    ] {
        assert!(why.to_string().contains(Arguments::USAGE), "{why}");
    }
}

/// Zero bots is no bots, so it is not the half of a conflict: a script that
/// passes `--bots $N` with `N` unset should reach the other machine rather than
/// be refused for asking for nothing.
#[test]
fn no_bots_and_a_peer_is_not_a_conflict() {
    let parsed = Arguments::parse(["--bots", "0", "--connect", "host:9001"])
        .expect("no bots is not bots and a peer");
    assert_eq!(parsed.num_bots, 0);
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

/// `--level` opens on the level it names — the reference *and* the content,
/// because the reference is hashed into nothing and a flag that moved only it
/// would rename the level a run is already on rather than choose one.
#[test]
fn a_level_this_game_has_is_what_the_run_opens_on() {
    let run = App::<Counting>::new()
        .opening(opening::<Tally>(Rules::quiet()))
        .arguments(
            Arguments::parse(["--headless", "--ticks=2", "--level", "\"meadow\""])
                .expect("a level reference is JSON of the game's own type"),
        )
        .run()
        .expect("this game builds that level from its name");

    assert_eq!(run.session.opening.level, "meadow");
    assert_eq!(
        run.session.opening.content.name, "meadow",
        "the content moved with the reference, so this run played the level it named",
    );
}

/// The two ways a `--level` is refused, which are two different things being
/// wrong and say so differently.
#[test]
fn a_level_this_game_cannot_open_on_is_refused() {
    // Not the game's reference at all: `Ref` is a `String`, and a bare word is
    // not JSON.
    let why = App::<Counting>::new()
        .opening(opening::<Tally>(Rules::quiet()))
        .arguments(Arguments::parse(["--level", "meadow"]).expect("the parser holds a string"))
        .run()
        .expect_err("a bare word is not this game's level reference");
    assert!(
        matches!(why, corvid_app::Error::Argument(Argument::NotALevel { .. })),
        "{why:?}",
    );

    // The game's reference, naming a level it reads out of files — which a
    // command line has none of to hand it. Refused, rather than opened on
    // whatever content the opening happened to carry.
    let why = App::<Counting>::new()
        .opening(opening::<Tally>(Rules::quiet()))
        .arguments(
            Arguments::parse(["--level", &format!("\"{}\"", common::ELSEWHERE)])
                .expect("the parser holds a string"),
        )
        .run()
        .expect_err("this game reads that level from files");
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
