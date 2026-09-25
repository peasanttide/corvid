//! The binding file: what a player changed about the controls a game ships.
//!
//! A game states the table it ships in
//! [`Controller::bindings`](corvid_control::Controller::bindings). That is the
//! author's answer, and it is the right one until somebody wants `Q` where the
//! game put `E`. This module reads the player's answer, which is laid over the
//! author's and wins wherever it says anything.
//!
//! # Only a windowed run
//!
//! A headless run has no devices, so which control raises which action decides
//! nothing about it. So the file is read from the windowed path and nowhere
//! else.
//!
//! # Never written
//!
//! The file holds what the player changed and nothing the game ships, so this
//! crate never writes it. A file holding the defaults would freeze them: a
//! later build could not move an action's control without the old one reading
//! as the player's choice, and an older build rewriting it could delete what
//! a newer one bound.

use std::fs;
use std::path::Path;

use corvid_input::SetDescriptor;
use corvid_input::platform::{Bindings, Table, Unknown};

use crate::app::Error;

/// What the file is called, inside a game's own state directory.
///
/// Beside `saves/` and the settings file rather than somewhere of its own,
/// because that directory is already this game's own and already redirectable
/// with `--state` -- which is what lets a test point this at a temporary
/// directory using an argument that already exists.
pub(crate) const FILE: &str = "bindings.json";

/// Why a binding file could not be used.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Misbound {
    /// It is not JSON, or not JSON of the shape a table is.
    #[error(transparent)]
    Shape(#[from] serde_json::Error),
    /// It is a table, and it names a control this build does not have, or
    /// gives an axis a span of zero.
    #[error(transparent)]
    Named(#[from] Unknown),
}

/// The table this run plays with: `shipped`, with the player's file laid over
/// it ([`Table::overlay`]) if there is one.
///
/// # Why a bad file stops the run
///
/// The alternative is to warn and carry on with the defaults, and the failure
/// mode of that is a key which silently does nothing and a player with no way
/// to find out why. A file that does not parse, or that names a control `Spcae`,
/// is a mistake somebody made in a text editor a minute ago and can fix -- if
/// they are told. So this refuses, and the message names the word that was
/// wrong.
///
/// # Why an unknown action does not
///
/// A game drops and renames actions between builds, and the file on a
/// player's disk outlives the build that was running when they wrote it. An
/// action name this build does not declare may be a typo or may be an action
/// the game dropped, and the file cannot say which; refusing would stop a game
/// from starting over its own rename. So its entries are left out, and the
/// names are reported through `tracing` for a player hunting a typo.
///
/// # Errors
///
/// [`Error::Read`](crate::Error::Read) if the file is there and will not be read, and
/// [`Error::Bound`](crate::Error::Bound) if it is read and cannot be used.
pub(crate) fn resolve(
    directory: &Path,
    sets: &[SetDescriptor],
    shipped: Bindings,
) -> Result<Bindings, Error> {
    let path = directory.join(FILE);
    match fs::read_to_string(&path) {
        Ok(text) => {
            let table: Table = serde_json::from_str(&text).map_err(|why| Error::Bound {
                path: path.clone(),
                why: Misbound::Shape(why),
            })?;
            let overlaid = table.overlay(sets, &shipped);
            if !overlaid.ignored.is_empty() {
                tracing::warn!(
                    name: "corvid_app.ignored_bindings",
                    path = %path.display(),
                    actions = ?overlaid.ignored,
                    "the binding file names actions this build does not declare, as the kind \
                     it binds them as; they are either misspelled or dropped from the game, \
                     and are left out of this run",
                );
            }
            if !overlaid.withheld.is_empty() {
                tracing::info!(
                    name: "corvid_app.withheld_bindings",
                    path = %path.display(),
                    controls = ?overlaid.withheld,
                    "controls the game ships are left off the actions it ships them on, \
                     because the binding file gives them to something else",
                );
            }
            let bindings = overlaid
                .table
                .to_bindings(sets)
                .map_err(|why| Error::Bound {
                    path: path.clone(),
                    why: Misbound::Named(why),
                })?;
            tracing::info!(
                name: "corvid_app.bound",
                path = %path.display(),
                buttons = bindings.buttons().len(),
                axes = bindings.axes().len(),
                "the player's binding file is laid over the table the game ships",
            );
            Ok(bindings)
        }
        // Not an error, and the common case: nobody has rebound anything.
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => {
            let names = |kind: fn(SetDescriptor) -> &'static [&'static str]| {
                sets.iter().flat_map(|set| kind(*set)).collect::<Vec<_>>()
            };
            tracing::info!(
                name: "corvid_app.shipped_bindings",
                path = %path.display(),
                digital = ?names(SetDescriptor::digital_names),
                analog = ?names(SetDescriptor::analog_names),
                "no binding file, so this run plays the table the game ships; a file at \
                 this path rebinds the actions named here",
            );
            Ok(shipped)
        }
        Err(why) => Err(Error::Read { path, why }),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use corvid_input::platform::{Button, Key};
    use corvid_input::{DigitalId, SetDescriptor, SetNames, layout};

    use super::*;

    /// A game with two actions and one axis to bind them against.
    static SETS: [SetDescriptor; 1] = layout(&[SetNames {
        name: "Playing",
        digital: &["JUMP", "DUCK"],
        analog: &["LOOK"],
        pose: &[],
    }]);

    /// A directory nothing else is using, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "corvid_app-controls-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            drop(fs::remove_dir_all(&path));
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    /// The table a game ships, for these tests: one control on one action.
    fn shipped() -> Bindings {
        Bindings::new().button(Button::key(Key::Space), DigitalId(0))
    }

    /// Writes `text` as the binding file in a fresh directory.
    fn with_file(text: &str) -> Scratch {
        let scratch = Scratch::new();
        fs::create_dir_all(&scratch.0).unwrap();
        fs::write(scratch.0.join(FILE), text).unwrap();
        scratch
    }

    #[test]
    fn a_run_with_no_file_plays_the_table_the_game_ships_and_writes_nothing() {
        let scratch = Scratch::new();
        let bound = resolve(&scratch.0, &SETS, shipped()).expect("nothing is there to refuse");
        assert_eq!(bound, shipped());
        assert!(
            !scratch.0.join(FILE).exists(),
            "defaults are not the player's"
        );
    }

    #[test]
    fn the_file_is_laid_over_the_table_the_game_ships() {
        let scratch = with_file(r#"{ "buttons": [{ "control": "Q", "action": "DUCK" }] }"#);
        let bound = resolve(&scratch.0, &SETS, shipped()).expect("it names things this build has");
        assert_eq!(
            bound.buttons(),
            [
                (Button::key(Key::Space), DigitalId(0)),
                (Button::key(Key::Q), DigitalId(1)),
            ],
            "JUMP kept what the game ships, and DUCK took what the player gave it",
        );
    }

    #[test]
    fn the_file_is_never_rewritten() {
        // Not even when it names an action this build lacks, which a newer or
        // older build may still declare.
        let text = r#"{ "buttons": [{ "control": "R", "action": "ROLL" }] }"#;
        let scratch = with_file(text);
        resolve(&scratch.0, &SETS, shipped()).expect("an unknown action is not fatal");
        assert_eq!(fs::read_to_string(scratch.0.join(FILE)).unwrap(), text);
    }

    #[test]
    fn an_action_this_build_does_not_declare_is_left_out_of_the_run() {
        let scratch = with_file(r#"{ "buttons": [{ "control": "Q", "action": "JMUP" }] }"#);
        let bound = resolve(&scratch.0, &SETS, shipped()).expect("dropped or typo, it plays");
        assert_eq!(bound, shipped());
    }

    #[test]
    fn a_control_this_build_does_not_name_stops_the_run() {
        // The typo is named, which is the whole reason this refuses rather than
        // falling back: a player who is told "Spcae" can fix it, and a player
        // whose key silently does nothing cannot.
        let scratch = with_file(r#"{ "buttons": [{ "control": "Spcae", "action": "JUMP" }] }"#);
        let why = resolve(&scratch.0, &SETS, shipped()).expect_err("a typo is not playable");
        let said = why.to_string();
        assert!(said.contains("Spcae"), "{said}");
        assert!(said.contains(FILE), "{said}");
    }

    #[test]
    fn a_file_that_is_not_a_table_stops_the_run_too() {
        let scratch = with_file("{ this is not json");
        let why = resolve(&scratch.0, &SETS, shipped()).expect_err("it does not parse");
        assert!(why.to_string().contains(FILE), "{why}");
    }

    #[test]
    fn a_span_of_zero_is_refused_rather_than_dividing_by_it() {
        let scratch = with_file(
            r#"{ "axes": [{ "control": "MouseMotion", "action": "LOOK",
                            "span": 0, "reading": "Displacement" }] }"#,
        );
        let why = resolve(&scratch.0, &SETS, shipped()).expect_err("zero cannot divide");
        assert!(why.to_string().contains("LOOK"), "{why}");
    }

    #[test]
    fn a_file_that_will_not_be_read_stops_the_run() {
        // Not the missing case: something is *there* and cannot be read, which
        // on every platform a directory in the file's place produces. A run
        // that quietly played on with the shipped table would be hiding a
        // broken installation, so this is reported with the path in it.
        let scratch = Scratch::new();
        fs::create_dir_all(scratch.0.join(FILE)).unwrap();
        let why = resolve(&scratch.0, &SETS, shipped()).expect_err("it cannot be read");
        assert!(matches!(why, Error::Read { .. }), "{why}");
        assert!(why.to_string().contains(FILE), "{why}");
    }
}
