//! Writing a session to one file, which is what `--demo` opens.

use std::path::Path;

use corvid_behavior::State;
use corvid_replay::Session;

use crate::Error;

/// Writes `session` to `path`.
///
/// The same bytes a capture's `session` file holds, so a `--record` and a
/// capture produce a file `--demo` and
/// [`replay`](crate::App::replay) read identically. The directory above the
/// file is created if it is not there, because `--record out/session` names a
/// file and an operator who named one did not also ask to make a directory for
/// it.
///
/// # Errors
///
/// [`Error::Encoded`] if the session will not encode, and [`Error::Wrote`] if
/// the file will not be written.
pub(crate) fn write<S: State>(path: &Path, session: &Session<S>) -> Result<(), Error> {
    let bytes = session.save().map_err(|why| Error::Encoded {
        what: "a session",
        why,
    })?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|why| Error::Wrote {
            path: parent.to_path_buf(),
            why,
        })?;
    }
    std::fs::write(path, &bytes).map_err(|why| Error::Wrote {
        path: path.to_path_buf(),
        why,
    })
}
