//! The binding file: where a player's own table is kept, and what happens when
//! it is not there.
//!
//! A game states the table it ships in
//! [`Present::bindings`](corvid_present::Present::bindings). That is the
//! author's answer, and it is the right one until somebody wants `Q` where the
//! game put `E`. This module is the player's answer, and it wins.
//!
//! # Only a windowed run
//!
//! A headless run has no devices, so which control raises which action decides
//! nothing about it — and a determinism check that wrote a file into a
//! directory as a side effect of running would be a determinism check with a
//! side effect. So the file is read and written from the windowed path and
//! nowhere else.

use std::fs;
use std::path::{Path, PathBuf};

use corvid_input::SetDescriptor;
use corvid_input::platform::{Bindings, Table, Unknown};

use crate::app::Error;

/// What the file is called, inside the directory the save slots are in.
///
/// Beside the saves rather than somewhere of its own, because that directory is
/// already this game's own, already created when it is needed, and already
/// redirectable with `--saves` — which is what lets a test point this at a
/// temporary directory using an argument that already exists.
pub(crate) const FILE: &str = "bindings.json";

/// Two spaces high, which is enough to see the shape of a line without the file
/// being mostly whitespace.
const INDENT: &[u8] = b"  ";

/// Why a binding file could not be used.
#[derive(Debug)]
#[non_exhaustive]
pub enum Misbound {
    /// It is not JSON, or not JSON of the shape a table is.
    Shape(serde_json::Error),
    /// It is a table, and it names something this build does not have.
    Named(Unknown),
}

impl std::fmt::Display for Misbound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shape(why) => write!(f, "{why}"),
            Self::Named(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for Misbound {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Shape(why) => Some(why),
            Self::Named(why) => Some(why),
        }
    }
}

/// The table this run plays with: the file if there is one, and `shipped`
/// written out for the player to edit if there is not.
///
/// # Why a bad file stops the run
///
/// The alternative is to warn and carry on with the defaults, and the failure
/// mode of that is a key which silently does nothing and a player with no way
/// to find out why. A file that does not parse, or that names `FOWARD`, is a
/// mistake somebody made in a text editor a minute ago and can fix — if they
/// are told. So this refuses, and the message names the word that was wrong.
///
/// # Why a missing file is written rather than ignored
///
/// It is the only way a player learns what the action names are. There is no
/// rebinding screen in this crate and no documentation shipped beside a binary,
/// so a file that appears the first time the game is run, holding exactly the
/// bindings the game is playing with, is the discoverable form of the feature.
///
/// A failure to *write* it is not a failure to run: the directory may be read
/// only, and a game that refused to start over that would be refusing to start
/// over something it wanted to do rather than something it was asked to do. It
/// is reported through `tracing` and the run plays on with `shipped`.
///
/// # Errors
///
/// [`Error::Read`] if the file is there and will not be read, and
/// [`Error::Bound`] if it is read and cannot be used.
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
            let bindings = table.to_bindings(sets).map_err(|why| Error::Bound {
                path: path.clone(),
                why: Misbound::Named(why),
            })?;
            tracing::info!(
                name: "corvid_app.bound",
                path = %path.display(),
                buttons = bindings.buttons().len(),
                axes = bindings.axes().len(),
                "the player's own binding file is what this run is bound by",
            );
            Ok(bindings)
        }
        // Not an error, and the common case: nobody has rebound anything yet.
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => {
            write(&path, sets, &shipped);
            Ok(shipped)
        }
        Err(why) => Err(Error::Read { path, why }),
    }
}

/// Writes the shipped table out, reporting rather than failing.
///
/// See [`resolve`] for why this cannot fail the run.
fn write(path: &PathBuf, sets: &[SetDescriptor], shipped: &Bindings) {
    let table = Table::from_bindings(shipped, sets);
    let written = path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| {
            let mut out = Vec::new();
            let mut writer = serde_json::Serializer::with_formatter(
                &mut out,
                serde_json::ser::PrettyFormatter::with_indent(INDENT),
            );
            serde::Serialize::serialize(&table, &mut writer)
                .map_err(std::io::Error::other)
                .map(|()| out)
        })
        .and_then(|mut bytes| {
            // A trailing newline, because this is a text file a person opens in
            // an editor and every other text file they have ends in one.
            bytes.push(b'\n');
            fs::write(path, bytes)
        });
    match written {
        Ok(()) => tracing::info!(
            name: "corvid_app.wrote_bindings",
            path = %path.display(),
            "nobody had rebound anything, so the table this game ships is now a file to edit",
        ),
        Err(why) => tracing::warn!(
            name: "corvid_app.unwritten_bindings",
            path = %path.display(),
            error = %why,
            "the binding file could not be written, so this run is bound by the table the \
             game ships and there is nothing on disk to edit",
        ),
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]
mod tests {
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

    #[test]
    fn a_run_with_no_file_writes_the_table_the_game_ships() {
        let scratch = Scratch::new();
        let bound = resolve(&scratch.0, &SETS, shipped()).expect("nothing is there to refuse");
        assert_eq!(bound, shipped(), "the shipped table is what this run plays");

        // And it is on disk, naming the action rather than its number, so a
        // player can open it and see what the words are.
        let written = fs::read_to_string(scratch.0.join(FILE)).expect("it was written");
        assert!(
            written.contains("JUMP"),
            "the file names the action: {written}"
        );
        assert!(written.contains("Space"), "and the control: {written}");
        assert!(
            !written.contains("\"action\": 0"),
            "by name and not by number"
        );
    }

    #[test]
    fn the_file_replaces_the_table_the_game_ships() {
        let scratch = Scratch::new();
        fs::create_dir_all(&scratch.0).unwrap();
        fs::write(
            scratch.0.join(FILE),
            r#"{ "buttons": [{ "control": "Q", "action": "DUCK" }] }"#,
        )
        .unwrap();

        let bound = resolve(&scratch.0, &SETS, shipped()).expect("it names things this build has");
        assert_eq!(
            bound.buttons(),
            [(Button::key(Key::Q), DigitalId(1))],
            "the player's file won, entirely rather than as an overlay",
        );
    }

    #[test]
    fn a_file_that_names_nothing_this_build_has_stops_the_run() {
        let scratch = Scratch::new();
        fs::create_dir_all(&scratch.0).unwrap();
        fs::write(
            scratch.0.join(FILE),
            r#"{ "buttons": [{ "control": "Q", "action": "JMUP" }] }"#,
        )
        .unwrap();

        // The typo is named, which is the whole reason this refuses rather than
        // falling back: a player who is told "JMUP" can fix it, and a player
        // whose key silently does nothing cannot.
        let why = resolve(&scratch.0, &SETS, shipped()).expect_err("a typo is not playable");
        let said = why.to_string();
        assert!(said.contains("JMUP"), "{said}");
        assert!(said.contains(FILE), "{said}");
    }

    #[test]
    fn a_file_that_is_not_a_table_stops_the_run_too() {
        let scratch = Scratch::new();
        fs::create_dir_all(&scratch.0).unwrap();
        fs::write(scratch.0.join(FILE), "{ this is not json").unwrap();
        let why = resolve(&scratch.0, &SETS, shipped()).expect_err("it does not parse");
        assert!(why.to_string().contains(FILE), "{why}");
    }

    #[test]
    fn a_span_of_zero_is_refused_rather_than_dividing_by_it() {
        let scratch = Scratch::new();
        fs::create_dir_all(&scratch.0).unwrap();
        fs::write(
            scratch.0.join(FILE),
            r#"{ "axes": [{ "control": "MouseMotion", "action": "LOOK",
                            "span": 0, "reading": "Displacement" }] }"#,
        )
        .unwrap();
        let why = resolve(&scratch.0, &SETS, shipped()).expect_err("zero cannot divide");
        assert!(why.to_string().contains("LOOK"), "{why}");
    }

    #[test]
    fn what_is_written_is_what_is_read_back() {
        // The round trip, over a table with the two shapes a map could not
        // hold: two controls on one action, and one control on two.
        let scratch = Scratch::new();
        let table = Bindings::new()
            .button(Button::key(Key::W), DigitalId(0))
            .button(Button::key(Key::ArrowUp), DigitalId(0))
            .button(Button::key(Key::W), DigitalId(1));
        let first = resolve(&scratch.0, &SETS, table.clone()).expect("nothing is there");
        assert_eq!(first, table);

        // Second run, same directory: the file it wrote is now the file it
        // reads, and it is the same table in the same order.
        let second = resolve(&scratch.0, &SETS, Bindings::new()).expect("it wrote it itself");
        assert_eq!(second, table);
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

    #[test]
    fn a_directory_that_will_not_take_the_file_does_not_stop_the_run() {
        // The other half, and the asymmetry is deliberate. Writing the defaults
        // out is something this does *for* the player rather than something it
        // was asked to do, so a directory that will not take it is a reason to
        // play without a file rather than a reason not to play. It is reported
        // through `tracing` and the run goes on.
        let scratch = Scratch::new();
        fs::create_dir_all(scratch.0.join(FILE)).unwrap();
        write(&scratch.0.join(FILE), &SETS, &shipped());
        assert!(
            scratch.0.join(FILE).is_dir(),
            "the directory is still a directory and nothing panicked",
        );
    }
}
