//! A source with no filesystem under it, for tests and for embedded data.

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};

use crate::{Missing, Source};

/// Files held in memory, keyed by path.
///
/// A `BTreeMap` rather than a hash map, and the reason is
/// [`list`](Source::list): a sorted map answers a prefix by walking a range
/// instead of by filtering everything, and it answers it *in order* — which the
/// trait requires, because a level whose props were walked in a map's
/// iteration order is a level two peers would hash differently.
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

    fn list(&self, prefix: &str) -> Result<Vec<String>, Missing> {
        // A range from the prefix, stopping at the first key that no longer
        // starts with it. Sorted order is what makes `take_while` correct here
        // rather than merely fast: every key sharing the prefix is contiguous.
        Ok(self
            .files
            .range(prefix.to_string()..)
            .take_while(|(path, _)| path.starts_with(prefix))
            .map(|(path, _)| path.clone())
            .collect())
    }
}
