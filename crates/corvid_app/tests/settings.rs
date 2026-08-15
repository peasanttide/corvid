//! The settings file: where it is, what it holds, and that it survives being
//! written down.
//!
//! What is **not** here is a test that reads or writes the real path. Where a
//! game's directory lands is a pure function of `$XDG_DATA_HOME`, `%APPDATA%`
//! and `$HOME`, and a test that moved any of them would be a test moving the
//! environment of every other test in the process -- `std::env::set_var` is
//! `unsafe`, which this workspace forbids rather than merely denies. So this
//! pins the parts that can be pinned without one: where the file lands under a
//! directory it is given, what the document holds, and that it round-trips.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use std::path::Path;

use common::{Bare, Counting, Holding};
use corvid_app::Settings;
use corvid_time::Tick;

/// The settings for the harness's game, with nothing set.
type Plain = Settings<Bare>;

/// The settings for the game the rest of this crate's tests play.
///
/// Its controller has a config with fields in it, which [`Plain`]'s four do not
/// -- every one of those is `()`, and a `()` that was read is indistinguishable
/// from a `()` that was defaulted. A test about *which* keys survived needs a
/// value that can tell the difference.
type Furnished = Settings<Counting>;

#[test]
fn the_file_sits_in_the_directory_it_is_given_and_is_called_setting_json() {
    // A join rather than a lookup: the game's own directory is resolved once
    // for a whole run -- `--state DIR` or the user's data home -- and the
    // settings file, the binding file and `saves/` are all under it. So there
    // is one answer to "where does this game write" and a test can name it.
    let root = Path::new("/somewhere/counter");
    assert_eq!(Plain::path(root), root.join("setting.json"));
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
fn the_four_configs_are_named_rather_than_positional() {
    // A JSON object with four keys, not an array of four values: a player
    // opens this file, and a document whose fields were positional would be one
    // where inserting a config silently reinterprets the other three.
    let text = serde_json::to_string(&Plain::default()).unwrap();

    for key in ["controls", "bot", "graphics", "audio"] {
        assert!(text.contains(&format!("\"{key}\"")), "{text}");
    }
}

#[test]
fn a_document_missing_a_key_keeps_the_keys_it_has() {
    // The failure this guards against: this document grows, and a build that
    // adds a setting would otherwise turn every existing player's file into
    // `Error::Setting` -- a refusal to start a run over a key nobody could have
    // written. So a file with one of the four in it reads back with that one as
    // written and the other three at their defaults.
    let written = Furnished {
        controls: Holding {
            pause_at: Some(Tick(4)),
            pause_for: 9,
        },
        ..Furnished::default()
    };

    // Built by dropping keys from a real document rather than by writing JSON
    // out by hand, so that this test says "three keys are absent" and not "the
    // configs are spelled the way I guessed".
    let mut document = serde_json::to_value(&written).unwrap();
    let object = document
        .as_object_mut()
        .expect("the settings are a JSON object");
    for key in ["bot", "graphics", "audio"] {
        assert!(object.remove(key).is_some(), "{document}");
    }

    let read: Furnished = serde_json::from_value(document).expect("a file with one key in it");
    assert_eq!(read, written);
}
