//! A source with no filesystem under it, for tests and for embedded data.

use alloc::{collections::BTreeMap, string::String, vec::Vec};

use crate::{Missing, ReadOnly, Source};

/// Files held in memory, keyed by path.
///
/// A `BTreeMap` rather than a hash map, and the reason is
/// [`list`](Source::list): the trait requires the listing to be in order,
/// because a level whose props were walked in a map's iteration order is a
/// level two peers would hash differently. A sorted map is already in that
/// order, so the listing is a walk rather than a walk and a sort.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Memory {
    files: BTreeMap<String, Vec<u8>>,
}

impl Memory {
    /// Nothing at all.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            files: BTreeMap::new(),
        }
    }

    /// Puts a file at a path, answering whatever was there.
    ///
    /// The map-shaped half of [`Source::write`], and the one to reach for when
    /// building a `Memory` up: it takes the bytes rather than copying them, and
    /// it hands back what it displaced, so a loader that overwrote a file it
    /// did not mean to is told at the call rather than at the next read.
    pub fn insert(&mut self, path: impl Into<String>, bytes: Vec<u8>) -> Option<Vec<u8>> {
        self.files.insert(path.into(), bytes)
    }

    /// Takes one away.
    pub fn remove(&mut self, path: &str) -> Option<Vec<u8>> {
        self.files.remove(path)
    }

    /// How many files are in it.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl From<BTreeMap<String, Vec<u8>>> for Memory {
    fn from(files: BTreeMap<String, Vec<u8>>) -> Self {
        Self { files }
    }
}

impl<K: Into<String>> FromIterator<(K, Vec<u8>)> for Memory {
    fn from_iter<I: IntoIterator<Item = (K, Vec<u8>)>>(entries: I) -> Self {
        Self {
            files: entries
                .into_iter()
                .map(|(path, bytes)| (path.into(), bytes))
                .collect(),
        }
    }
}

impl Source for Memory {
    fn read(&self, path: &str) -> Result<Vec<u8>, Missing> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| Missing::new(path))
    }

    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn list(&self) -> Result<Vec<String>, Missing> {
        // Already in order, because the map is. Nothing here can fail: a map
        // that holds no files still answers the question, and the `Result` is
        // the trait's, for the sources that have a directory under them.
        Ok(self.files.keys().cloned().collect())
    }

    // Infallible, and the `Ok` is the trait's shape rather than an outcome
    // worth checking: a map always has room. `insert`'s displaced bytes are
    // dropped, because a caller that wanted them would have called `insert`.
    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), ReadOnly> {
        self.files.insert(path.into(), bytes.to_vec());
        Ok(())
    }
}
