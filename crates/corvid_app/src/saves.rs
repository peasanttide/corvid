//! Save slots: where they live, what one holds, and what reading one does.
//!
//! A game implements nothing for any of this. Its `State` is
//! [`Data`](corvid_behavior::Data) and its session is already the whole of what
//! happened, so writing a save is writing those two down and reading one is
//! [`Session::seek`].

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use corvid_behavior::{SaveSlot, State};
use corvid_hash::{Digest, digest};
use corvid_replay::{Session, Snapshots};
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

use crate::Error;

/// The leaf a game's slots sit in, under its own directory in the user's data
/// home.
///
/// A leaf rather than the whole path, because the state root is
/// `$XDG_DATA_HOME/<name>/` and saves are one kind of thing a game keeps
/// there — the settings file and the binding file are the others, and they want
/// to be siblings rather than to be mixed in among the slots.
const SAVES: &str = "saves";

/// Where a user's own data lives, per the XDG Base Directory specification.
///
/// [`None`] when the environment names nowhere, which is a login shell with no
/// `HOME` and a service account. The caller falls back to a relative path
/// there, because refusing to run is the wrong answer to "this machine has an
/// unusual environment".
///
/// # Why this is written out rather than taken from a crate
///
/// It is the spec, and the spec is short: an absolute `XDG_DATA_HOME` wins,
/// and `$HOME/.local/share` is the documented default when it is unset or
/// relative. Windows has no `XDG_DATA_HOME` to read and `%APPDATA%` is the
/// answer there, which is the one platform branch this needs.
fn data_home() -> Option<PathBuf> {
    // "If $XDG_DATA_HOME is either not set or empty, a default equal to
    // $HOME/.local/share should be used." The spec also requires the value to
    // be an absolute path and says relative ones are to be ignored.
    if let Some(set) = std::env::var_os("XDG_DATA_HOME") {
        let path = PathBuf::from(set);
        if path.is_absolute() {
            return Some(path);
        }
    }
    // Read before `HOME` rather than after it, because a Windows shell that has
    // both — MSYS and Git Bash both set `HOME` — means the Unix one for its own
    // programs and `%APPDATA%` for everything else.
    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let path = PathBuf::from(appdata);
        if path.is_absolute() {
            return Some(path);
        }
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    home.is_absolute()
        .then(|| home.join(".local").join("share"))
}

/// Everything one game keeps between runs, in one directory.
///
/// `--state DIR` if the operator named one, and `$XDG_DATA_HOME/NAME/`
/// otherwise — so `~/.local/share/NAME/` on a machine that has not set it, and
/// `%APPDATA%\NAME\` on Windows. Under it are `saves/`, the settings file and
/// the binding file.
///
/// **One directory rather than three**, and that is the whole of why this
/// function exists: a player who wants to move a game to another machine, back
/// it up, or throw it away copies one path, and a test that wants a run to
/// touch nothing of theirs redirects one flag. The specification does keep
/// configuration and data apart, and the split is real for a program whose
/// configuration is edited by something other than the program; here the
/// settings file, the binding file and the slots are all written by the game
/// and read by the same game, and separating them would mean a `--state` that
/// redirected some of what a run writes.
///
/// # The machine that names no home
///
/// `./NAME/`, beside wherever the game was launched from — which is a decision
/// and is worth saying out loud, because it means a **settings file gets
/// written into the working directory** on such a machine, where a run that
/// resolved the settings path on its own could answer "nowhere" and quietly use
/// the defaults.
///
/// It is the right answer once there is one root. A game that saved somebody's
/// hour into `./NAME/saves/` and refused to remember the volume they set would
/// be making two different judgements about the same directory; and the run
/// that hits this at all is a login shell with no `HOME` or a service account,
/// where refusing to start is worse than writing where it was told. An operator
/// who does not want files there passes `--state`.
pub(crate) fn root(named: Option<PathBuf>, name: &str) -> PathBuf {
    resolve(data_home(), named, name)
}

/// [`root`] with the environment handed to it rather than read.
///
/// Split out so that all three answers are testable: `std::env::set_var` is
/// `unsafe`, which this workspace forbids, so a test that moved `XDG_DATA_HOME`
/// would be a test moving the environment of every other test in the process.
fn resolve(home: Option<PathBuf>, named: Option<PathBuf>, name: &str) -> PathBuf {
    named.unwrap_or_else(|| home.map_or_else(|| PathBuf::from(name), |home| home.join(name)))
}

/// What one slot's file is called.
const EXTENSION: &str = "corvid";

/// The second extension a slot's bytes wear while they are being written, before
/// they are renamed over the slot itself.
///
/// Beside the slot rather than in a temporary directory, because a rename is
/// only atomic within one filesystem and the system's temporary directory is
/// routinely on another one.
const PENDING: &str = "new";

/// A session out of a slot, and the state to play it from.
///
/// A pair rather than a struct, and a name rather than the pair written out
/// four times: it is what every reader of a slot wants back.
///
/// The state is a handle because [`Session::seek`] hands one back and because
/// the loop that receives it holds one — so a save opens without the state
/// being copied on the way from the seek to the first frame.
pub(crate) type Resumed<S> = (Session<S>, Arc<S>);

/// A state and the tick it is the state at.
///
/// The tick is [`Session::last`] of the session that came out of the slot, kept
/// beside the state because the loop needs both and neither is derivable from
/// the other once the session has been handed over.
pub(crate) type StateAt<S> = (Tick, Arc<S>);

/// Where a run's save slots are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Saves {
    /// The directory the slot files sit in.
    root: PathBuf,
}

impl Saves {
    /// The `saves/` directory under a game's [`root`], which is where the slot
    /// files go.
    ///
    /// A leaf of the state root rather than a path of its own, so that
    /// `--state DIR` moves the slots along with the settings and the bindings:
    /// one directory is what a player copies to another machine, and a flag
    /// that moved two of the three would be a flag that half worked.
    pub(crate) fn under(root: &Path) -> Self {
        Self {
            root: root.join(SAVES),
        }
    }

    /// Where slot `slot` is written.
    fn path(&self, slot: SaveSlot) -> PathBuf {
        self.root.join(format!("{}.{EXTENSION}", slot.0))
    }

    /// Where slot `slot`'s bytes are assembled before they become the slot.
    ///
    /// A second extension rather than a different stem, so that
    /// [`path`](Self::path) and this can never name the same file and a
    /// half-written save is never mistaken for a slot by
    /// [`holds`](Self::holds).
    fn pending(&self, slot: SaveSlot) -> PathBuf {
        self.root.join(format!("{}.{EXTENSION}.{PENDING}", slot.0))
    }

    /// Writes `session` and the state at its last tick into `slot`.
    ///
    /// The directory is created if it is not there. An existing slot is
    /// overwritten, which is what saving over a slot means.
    ///
    /// # Why the bytes go somewhere else first
    ///
    /// A write that opened the slot and truncated it would destroy the save it
    /// was replacing before it knew whether it could produce the new one, and a
    /// disk that fills up or a process that is killed halfway through would
    /// leave somebody's hour-long run as the prefix of a save that never
    /// finished. So the bytes are written to a file beside the slot and renamed
    /// over it, and a rename within one directory is atomic on every platform
    /// this targets: the slot is either the save that was there or the save
    /// being written, and never a mixture of the two.
    ///
    /// `capture.rs` writes its files the plain way, deliberately. A capture is a
    /// recording of a run that can be made again — the run is reproducible, that
    /// being the whole point of it — so a torn capture costs a rerun, where a
    /// torn save costs the only copy of something that happened once.
    ///
    /// # Errors
    ///
    /// [`Error::Wrote`] if the directory or the file will not take it, and
    /// [`Error::Encoded`] if the session or the state will not serialize.
    pub(crate) fn write<S: State>(
        &self,
        slot: SaveSlot,
        session: &Session<S>,
        state: &S,
    ) -> Result<(), Error> {
        fs::create_dir_all(&self.root).map_err(|why| Error::Wrote {
            path: self.root.clone(),
            why,
        })?;
        let written = Written {
            session: session.save().map_err(|why| Error::Encoded {
                what: "a session",
                why,
            })?,
            state: corvid_wire::encode(state).map_err(|why| Error::Encoded {
                what: "a state",
                why,
            })?,
        };
        let bytes = corvid_wire::encode(&written).map_err(|why| Error::Encoded {
            what: "a save",
            why,
        })?;
        let pending = self.pending(slot);
        fs::write(&pending, &bytes).map_err(|why| Error::Wrote {
            path: pending.clone(),
            why,
        })?;
        let path = self.path(slot);
        fs::rename(&pending, &path).map_err(|why| {
            // The slot is untouched, so the only thing left over is the file
            // that was going to become it. Removing it is best-effort: a
            // rename that failed for a reason a remove also fails for leaves a
            // stray `.new` beside the slot, which the next save overwrites and
            // which `holds` and `read` both ignore.
            drop(fs::remove_file(&pending));
            Error::Wrote { path, why }
        })
    }

    /// Whether there is a save in `slot`.
    ///
    /// The file's existence and not its contents: a run that asked this once a
    /// tick would otherwise replay a whole session once a tick.
    ///
    /// # Errors
    ///
    /// [`Error::Read`] for a directory that will not say.
    pub(crate) fn holds(&self, slot: SaveSlot) -> Result<bool, Error> {
        let path = self.path(slot);
        match path.try_exists() {
            Ok(there) => Ok(there),
            Err(why) => Err(Error::Read { path, why }),
        }
    }

    /// Reads `slot` back, or [`None`] for a slot nothing has written.
    ///
    /// `schema` is the running build's, and a slot recorded by a build that
    /// describes its types differently is refused here rather than replayed
    /// into a different game.
    ///
    /// # Errors
    ///
    /// [`Error::Read`] if the file is there and will not open, and
    /// [`Error::Saved`] for a file that is not a save this build can play.
    pub(crate) fn read<S: State>(
        &self,
        slot: SaveSlot,
        schema: Digest,
    ) -> Result<Option<Resumed<S>>, Error> {
        let path = self.path(slot);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(why) if why.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(why) => return Err(Error::Read { path, why }),
        };
        let saved = open::<S>(&bytes, schema).map_err(|why| Error::Saved { path, why })?;
        Ok(Some(saved))
    }
}

/// A saved session, and the state it was saved at.
///
/// Two encoded blobs rather than two values, so that the session goes back
/// through [`Session::load`] — which is what compares the schema and checks the
/// session's parts against each other — rather than through a bare decode.
#[derive(Serialize, Deserialize)]
struct Written {
    /// The session, as [`Session::save`] wrote it.
    session: Vec<u8>,
    /// The state at [`Session::last`].
    state: Vec<u8>,
}

/// Turns a slot's bytes into the session it recorded and the state to play it
/// from.
///
/// The state is re-derived with [`Session::seek`] rather than taken from the
/// file, because seeking is what load, replay, rollback and time-walk all are
/// and a save that took a shortcut past it would be the one of the five that
/// was not tested. What the recorded state is for is the comparison: a build
/// whose arithmetic moved without its schema moving replays the same log to a
/// different state, and that is [`NotASave::Diverged`] here rather than a
/// divergence noticed by a peer an hour later.
fn open<S: State>(bytes: &[u8], schema: Digest) -> Result<Resumed<S>, NotASave> {
    let written: Written = corvid_wire::decode(bytes).map_err(NotASave::Bytes)?;
    let session = Session::<S>::load(&written.session, schema).map_err(NotASave::Session)?;
    let recorded: S = corvid_wire::decode(&written.state).map_err(NotASave::Bytes)?;

    // A budget of zero, because this ring is thrown away on the next line: the
    // one seek it serves replays from the opening whatever it holds.
    let mut snapshots = Snapshots::<S>::new(0);
    let (state, _scratch) = session
        .seek(&mut snapshots, session.last())
        .map_err(NotASave::Unreachable)?;
    // One side is a bare state off the file and the other is the handle the
    // seek produced, and the two digests are still comparable: `Hash` for an
    // `Arc<T>` forwards to `T`, so the hasher sees the same bytes either way.
    // If that stopped being true this comparison would refuse every save on
    // disk, which is why it is said out loud here rather than assumed.
    let (recorded, replayed) = (digest(&recorded), digest(&state));
    if recorded != replayed {
        return Err(NotASave::Diverged { recorded, replayed });
    }
    Ok((session, state))
}

/// Reads the session a `--record` wrote and the state it ends at.
///
/// The file is what [`record::write`](crate::record::write) put there, which is
/// the same bytes a capture directory's `session` holds: a bare [`Session`]
/// rather than a slot's two blobs, so this is the same seek with no recorded
/// state to compare against.
///
/// # Errors
///
/// [`Error::Read`] if the file will not open, and [`Error::Saved`] for a file
/// that is not a session this build can play.
pub(crate) fn recorded<S: State>(path: &Path, schema: Digest) -> Result<Resumed<S>, Error> {
    let bytes = fs::read(path).map_err(|why| Error::Read {
        path: path.to_path_buf(),
        why,
    })?;
    let read = || {
        let session = Session::<S>::load(&bytes, schema).map_err(NotASave::Session)?;
        let mut snapshots = Snapshots::<S>::new(0);
        let (state, _scratch) = session
            .seek(&mut snapshots, session.last())
            .map_err(NotASave::Unreachable)?;
        Ok((session, state))
    };
    read().map_err(|why| Error::Saved {
        path: path.to_path_buf(),
        why,
    })
}

/// A slot's bytes are not a save this build can play.
#[derive(Debug)]
#[non_exhaustive]
pub enum NotASave {
    /// The file is not a save, or the state inside it is not this game's.
    Bytes(corvid_wire::Error),
    /// The session inside it is not one this build can replay.
    Session(corvid_replay::Load),
    /// The session inside it does not reach its own last tick, which means the
    /// log and the opening in it disagree about where the session is.
    Unreachable(corvid_replay::Unreachable),
    /// Replaying the session produced a different state than the one saved
    /// beside it.
    ///
    /// The schema matched, so the two builds describe their types the same way
    /// and one of them computes something else out of them. That is the failure
    /// a schema digest cannot see, and it is worth refusing at the load rather
    /// than carrying into a session two peers will disagree about.
    Diverged {
        /// The digest of the state the save recorded.
        recorded: Digest,
        /// The digest of the state replaying its log arrives at.
        replayed: Digest,
    },
}

impl fmt::Display for NotASave {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bytes(why) => write!(f, "these are not the bytes of a save: {why}"),
            Self::Session(why) => write!(f, "{why}"),
            Self::Unreachable(why) => write!(
                f,
                "the session in this save does not reach its own last tick: {why}"
            ),
            Self::Diverged { recorded, replayed } => write!(
                f,
                "this save records the state at its last tick as {recorded} and \
                 replaying its own log arrives at {replayed}: the build that \
                 wrote it describes its types exactly as this one does and \
                 computes something else out of them"
            ),
        }
    }
}

impl std::error::Error for NotASave {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Bytes(why) => Some(why),
            Self::Session(why) => Some(why),
            Self::Unreachable(why) => Some(why),
            Self::Diverged { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a failed assertion in a test is a failed test, which is what a test is for"
    )]

    use super::{Saves, data_home, resolve, root};
    use corvid_behavior::SaveSlot;
    use std::path::{Path, PathBuf};

    /// A game's name, for the joins below.
    const NAME: &str = "counter";

    #[test]
    fn a_named_directory_is_the_root_whatever_the_environment_says() {
        // `--state DIR` is the operator's answer and nothing looks anywhere
        // else, which is what makes it usable from a test: a run told where to
        // put its files touches nothing of the developer's.
        let named = PathBuf::from("scratch/here");
        assert_eq!(
            resolve(Some(PathBuf::from("/data")), Some(named.clone()), NAME),
            named,
        );
        assert_eq!(resolve(None, Some(named.clone()), NAME), named);
    }

    #[test]
    fn the_default_root_is_the_data_home_and_the_games_name() {
        assert_eq!(
            resolve(Some(PathBuf::from("/data")), None, NAME),
            Path::new("/data").join(NAME),
        );
    }

    #[test]
    fn a_machine_that_names_no_home_writes_beside_itself() {
        // The documented fallback, and the one arm that cannot be reached by
        // running on a developer's machine: a relative directory named for the
        // game. Everything a run keeps is under it, settings included.
        assert_eq!(resolve(None, None, NAME), PathBuf::from(NAME));
    }

    #[test]
    fn the_environment_the_real_resolution_reads_is_the_data_home() {
        // The environment is not moved — `std::env::set_var` is `unsafe`, which
        // this workspace forbids — so what is asserted is that `root` is
        // `resolve` over `data_home()`, whichever of the two answers this
        // machine gives.
        assert_eq!(root(None, NAME), resolve(data_home(), None, NAME));
        // And that the answer is a directory named for the game, absolute
        // wherever there is a home to be absolute under.
        let resolved = root(None, NAME);
        assert!(resolved.ends_with(NAME), "{}", resolved.display());
        assert_eq!(
            resolved.is_absolute(),
            data_home().is_some(),
            "{}",
            resolved.display(),
        );
    }

    #[test]
    fn the_slots_are_a_leaf_of_the_root() {
        // Half of the claim `--state` rests on: the slots are under the root
        // rather than resolved apart from it. `tests/settings.rs` is the other
        // half, since a `Settings` needs a game to name and this module has
        // none.
        let root = Path::new("/data/counter");
        assert_eq!(
            Saves::under(root).path(SaveSlot(2)),
            root.join("saves").join("2.corvid"),
        );
    }
}
