#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

//! Help and completion are generated, not written.

use corvid_dev::Argument;
use corvid_dev::{Console, Invalid, Parameter, Reply};

use corvid_time::Tick;
/// A bespoke parameter, in the four lines the trait asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Speed {
    Slow,
    Fast,
}

impl Argument for Speed {
    const TYPE: &'static str = "speed";
    const CANDIDATES: &'static [&'static str] = &["fast", "slow"];

    fn parse(text: &str) -> Option<Self> {
        match text {
            "slow" => Some(Self::Slow),
            "fast" => Some(Self::Fast),
            _ => None,
        }
    }
}

fn registry() -> Console {
    let mut console = Console::new();
    console
        .register(
            "wave.skip",
            "advance to the next wave",
            |to: Option<u32>| Reply::said(format!("{to:?}")),
        )
        .named(&["to"]);
    console
        .register("wave.seek", "go to a tick", |to: Tick| {
            Reply::said(to.0.to_string())
        })
        .named(&["to"]);
    console
        .register("time.speed", "how fast", |speed: Speed| {
            Reply::said(format!("{speed:?}"))
        })
        .named(&["speed"]);
    console.register("time.pause", "stop the clock", || Reply::Done);
    console
}

#[test]
fn an_optional_parameter_generates_square_brackets() {
    let console = registry();

    let help = console.help(Some("wave.skip"));
    assert_eq!(help.len(), 1);
    assert_eq!(help[0].usage, "wave.skip [to: u32]");
    assert_eq!(help[0].help, "advance to the next wave");
}

#[test]
fn a_required_parameter_generates_angle_brackets_and_is_missed_by_name() {
    let mut console = registry();

    assert_eq!(
        console.help(Some("wave.seek"))[0].usage,
        "wave.seek <to: tick>"
    );
    assert_eq!(
        console.run("wave.seek").refusal(),
        Some(&Invalid::Missing {
            parameter: "to",
            of: "tick",
        }),
    );
    assert_eq!(console.run("wave.seek 40"), Reply::said("40"));
}

#[test]
fn a_repeated_parameter_says_so_and_takes_the_rest() {
    let mut console = Console::new();
    console
        .register("say", "repeat words", |words: Vec<String>| {
            Reply::said(words.join("-"))
        })
        .named(&["words"]);

    assert_eq!(console.help(None)[0].usage, "say [words: text...]");
    assert_eq!(
        console.run("say one two three"),
        Reply::said("one-two-three")
    );
    assert_eq!(console.run("say"), Reply::said(""));
}

#[test]
fn completion_under_a_prefix_offers_every_command_with_its_help() {
    let console = registry();

    let under = console.complete("wave.");
    assert_eq!(under.len(), 2);
    assert_eq!(under[0].text, "wave.seek");
    assert_eq!(under[0].help, "go to a tick");
    assert_eq!(under[1].text, "wave.skip");

    // And an empty prefix offers everything.
    assert_eq!(console.complete("").len(), 4);
}

#[test]
fn completion_on_a_parameter_offers_its_candidates() {
    let console = registry();

    let all = console.complete("time.speed ");
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].text, "fast");
    assert_eq!(all[0].help, "speed");

    let narrowed = console.complete("time.speed s");
    assert_eq!(narrowed.len(), 1);
    assert_eq!(narrowed[0].text, "slow");

    // A command that takes nothing offers nothing.
    assert!(console.complete("time.pause ").is_empty());
}

#[test]
fn an_unknown_command_answers_and_suggests_nothing() {
    let mut console = registry();

    assert_eq!(
        console.run("wave.skop"),
        Reply::Refused(Invalid::Unknown {
            path: "wave.skop".to_owned(),
        }),
    );
    // No fuzzy match: a console that runs the wrong command is worse than one
    // that runs none.
    assert!(console.complete("wave.skop").is_empty());
}

#[test]
fn too_many_words_are_refused_rather_than_ignored() {
    let mut console = registry();

    assert_eq!(
        console.run("wave.skip 3 4 5").refusal(),
        Some(&Invalid::Extra { words: 2 }),
    );
    assert_eq!(
        console.run("time.pause now").refusal(),
        Some(&Invalid::Extra { words: 1 }),
    );
}

#[test]
fn a_bad_word_is_refused_by_its_parameter_and_its_type() {
    let mut console = registry();

    assert_eq!(
        console.run("time.speed brisk").refusal(),
        Some(&Invalid::Malformed {
            parameter: "speed",
            of: "speed",
            given: "brisk".to_owned(),
        }),
    );
}

#[test]
fn help_for_the_whole_registry_is_sorted_by_path() {
    let console = registry();

    let paths: Vec<&str> = console
        .help(None)
        .into_iter()
        .map(|line| line.path)
        .collect();
    assert_eq!(
        paths,
        ["time.pause", "time.speed", "wave.seek", "wave.skip"]
    );

    // And `entries` is the same order, because it is the same vector.
    let listed: Vec<&str> = console.entries().iter().map(|entry| entry.path).collect();
    assert_eq!(listed, paths);
}

#[test]
fn an_empty_line_does_nothing() {
    let mut console = registry();

    assert_eq!(console.run(""), Reply::Done);
    assert_eq!(console.run("   "), Reply::Done);
}

#[test]
fn registering_a_path_twice_replaces_it() {
    let mut console = Console::new();
    console.register("a", "first", || Reply::said("first"));
    console.register("a", "second", || Reply::said("second"));

    assert_eq!(console.len(), 1);
    assert_eq!(console.run("a"), Reply::said("second"));
    assert_eq!(console.help(None)[0].help, "second");
}

/// `REQUIRED` is what puts the brackets in a usage line, and `take` is what
/// decides whether a line without the word is legal. A type where the two
/// disagree would generate help that lies.
#[test]
fn required_agrees_with_what_an_empty_line_takes() {
    fn agree<A: Argument + core::fmt::Debug>() {
        assert_eq!(
            A::REQUIRED,
            A::take(&[]).is_none(),
            "{} disagrees with itself",
            A::TYPE,
        );
    }

    agree::<u8>();
    agree::<u32>();
    agree::<u64>();
    agree::<i32>();
    agree::<usize>();
    agree::<isize>();
    agree::<char>();
    agree::<bool>();
    agree::<String>();
    agree::<Tick>();
    agree::<corvid_fixed::I16F16>();
    agree::<Option<u32>>();
    agree::<Vec<u32>>();
}

#[test]
fn a_parameter_is_built_from_its_type_alone() {
    assert_eq!(
        Parameter::of::<bool>("on"),
        Parameter {
            name: "on",
            of: "bool",
            required: true,
            repeated: false,
            candidates: &["false", "true"],
        },
    );
    assert_eq!(
        Parameter::of::<Vec<bool>>("on").to_string(),
        "[on: bool...]"
    );
}

#[test]
fn four_parameters_are_taken_in_order() {
    let mut console = Console::new();
    console
        .register(
            "set",
            "four of them",
            |a: u8, b: bool, c: String, d: Option<u32>| Reply::said(format!("{a} {b} {c} {d:?}")),
        )
        .named(&["a", "b", "c", "d"]);

    assert_eq!(
        console.help(None)[0].usage,
        "set <a: u8> <b: bool> <c: text> [d: u32]"
    );
    assert_eq!(console.run("set 7 true hi"), Reply::said("7 true hi None"));
    assert_eq!(
        console.run("set 7 true hi 9"),
        Reply::said("7 true hi Some(9)")
    );
}
