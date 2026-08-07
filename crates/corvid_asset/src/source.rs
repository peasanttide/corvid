//! The one source that needs an operating system.
//!
//! [`Source`], [`Missing`], [`Malformed`] and [`Memory`] all live in
//! `corvid_files`, below `std`, because `corvid_behavior` names the trait in
//! [`Level::load`](corvid_behavior::Level::load)'s signature and that crate is
//! `no_std`. What is left here is the implementation that opens a directory,
//! which is the one that could never have gone down there.

use std::path::{Component, Path, PathBuf};

use corvid_files::{Missing, Source};

/// The production source: a directory on disk.
///
/// A path is resolved *under* the root and nowhere else. A `..` component, a
/// leading `/` and a drive prefix are all refused rather than followed, so a
/// level name that arrived over a network cannot name a file outside the
/// directory the game was pointed at.
///
/// ```
/// use corvid_asset::Files;
/// use corvid_asset::Source;
///
/// let files = Files::new("assets");
/// assert!(!files.exists("../../etc/passwd"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Files {
    root: PathBuf,
}

impl Files {
    /// A source rooted at `root`.
    #[must_use]
    #[inline]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The directory every path is resolved under.
    #[must_use]
    #[inline]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where `path` lands under the root, or [`None`] if it would leave it.
    fn under(&self, path: &str) -> Option<PathBuf> {
        let mut full = self.root.clone();
        let mut named = false;
        for part in Path::new(path).components() {
            match part {
                Component::Normal(name) => {
                    full.push(name);
                    named = true;
                }
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        named.then_some(full)
    }
}

impl From<PathBuf> for Files {
    #[inline]
    fn from(root: PathBuf) -> Self {
        Self { root }
    }
}

impl Source for Files {
    fn read(&self, path: &str) -> Result<Vec<u8>, Missing> {
        self.under(path)
            .and_then(|full| std::fs::read(full).ok())
            .ok_or_else(|| Missing::new(path))
    }

    /// Every entry directly under `prefix`, sorted.
    ///
    /// `prefix` names a directory here rather than a string prefix, because
    /// that is what a filesystem can answer cheaply — and the paths handed back
    /// carry the prefix, so what comes out of this goes straight back into
    /// [`read`](Source::read).
    ///
    /// **Sorted**, which the trait requires and a filesystem does not provide:
    /// `read_dir` answers in whatever order the directory is stored in, and two
    /// peers whose levels listed their props differently would hash two
    /// different levels.
    ///
    /// A directory that is not there lists nothing, rather than failing. A
    /// level with no props is a level, and the alternative is every game
    /// checking before it asks. What does fail is a root that refuses to say
    /// what is in it at all.
    fn list(&self, prefix: &str) -> Result<Vec<String>, Missing> {
        let Some(directory) = self.under(prefix) else {
            return Err(Missing::new(prefix));
        };
        if !directory.is_dir() {
            return Ok(Vec::new());
        }
        let entries = std::fs::read_dir(&directory).map_err(|_| Missing::new(prefix))?;
        let mut found: Vec<String> = entries
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
            .map(|name| {
                if prefix.is_empty() || prefix.ends_with('/') {
                    format!("{prefix}{name}")
                } else {
                    format!("{prefix}/{name}")
                }
            })
            .collect();
        found.sort();
        Ok(found)
    }

    fn exists(&self, path: &str) -> bool {
        self.under(path).is_some_and(|full| full.is_file())
    }
}
