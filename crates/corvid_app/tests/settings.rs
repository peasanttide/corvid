//! The settings file: where it is, what it holds, and that it survives being
//! written down.
//!
//! What is **not** here is a test that reads or writes the real path. That
//! function is a pure function of `$XDG_CONFIG_HOME`, `%APPDATA%` and `$HOME`,
//! and a test that moved any of them would be a test moving the environment of
//! every other test in the process — `std::env::set_var` is `unsafe`, which this
//! workspace forbids rather than merely denies. So this pins the parts that can
//! be pinned without one: where the path lands relative to a home, what the
//! document holds, and that it round-trips.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::Tally;
use corvid_app::Settings;

/// The settings for the harness's game, with nothing set.
type Plain = Settings<Tally, (), (), ()>;

#[test]
fn the_file_is_named_for_the_game_and_ends_in_setting_json() {
    // `None` only where the environment names no home at all, which is not the
    // case on any machine this runs on.
    let path = Plain::path("counter").expect("a test machine has a home directory");

    assert!(path.ends_with("counter/setting.json"), "{}", path.display());
    // Absolute, because every one of the three sources is required to be — a
    // relative one is ignored by the resolver rather than joined onto the
    // working directory, which would put a player's settings wherever the game
    // happened to be started from.
    assert!(path.is_absolute(), "{}", path.display());
}

#[test]
fn the_document_survives_being_written_down_and_read_back() {
    // The one thing about this file that can break silently. A field renamed on
    // a config, or a `#[serde(skip)]` added to one, reads back as the default
    // and the player sees every setting reset with nothing saying why.
    let settings = Plain::default();
    let text = serde_json::to_string_pretty(&settings).unwrap();
    let back: Plain = serde_json::from_str(&text).unwrap();

    assert_eq!(back, settings);
}

#[test]
fn the_three_configs_are_named_rather_than_positional() {
    // A JSON object with three keys, not an array of three values: a player
    // opens this file, and a document whose fields were positional would be one
    // where inserting a config silently reinterprets the other two.
    let text = serde_json::to_string(&Plain::default()).unwrap();

    for key in ["controls", "graphics", "audio"] {
        assert!(text.contains(&format!("\"{key}\"")), "{text}");
    }
}
