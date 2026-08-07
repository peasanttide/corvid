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
/// A leaf rather than the whole path, because the data home is
/// `$XDG_DATA_HOME/<name>/` and saves are one kind of thing a game keeps
/// there — a capture and a binding file are others, and they want to be
/// siblings rather than to be mixed in among the slots.
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
    /// `--saves DIR` if the operator named one, and the user's own data
    /// directory otherwise.
    ///
    /// That is `$XDG_DATA_HOME/NAME/saves/` — so `~/.local/share/NAME/saves/`
    /// on a machine that has not set it, and `%APPDATA%\NAME\saves\` on
    /// Windows. A game's saves belong with the user's other data rather than
    /// beside whatever directory it happened to be launched from, which is
    /// where they used to land.
    ///
    /// The old relative `./saves/NAME/` survives as the fallback for an
    /// environment that names no home at all. Nothing that runs a game
    /// ordinarily hits it, and a run refusing to start because a service
    /// account has no `HOME` would be the worse answer.
    pub(crate) fn resolve(named: Option<PathBuf>, name: &str) -> Self {
        let root = named.unwrap_or_else(|| {
            data_home().map_or_else(
                || Path::new(SAVES).join(name),
                |home| home.join(name).join(SAVES),
            )
        });
        Self { root }
    }

    /// The directory itself.
    ///
    /// The binding file lives in it, beside the slots: it is already this
    /// game's own directory, already redirectable with `--saves`, and already
    /// created when something is written into it.
    #[cfg(feature = "window")]
    pub(crate) fn root(&self) -> &Path {
        &self.root
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

/// Reads the session a `--capture` wrote and the state it ends at.
///
/// The file is the `session` a capture directory holds, which is a bare
/// [`Session`] rather than a slot's two blobs — so this is the same seek with
/// no recorded state to compare against.
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
