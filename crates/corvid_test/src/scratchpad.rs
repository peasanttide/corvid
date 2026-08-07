//! A directory to capture into that nothing else is using.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// A directory under the system's temporary one, removed when this is dropped.
///
/// A capture is written somewhere and read back, and the somewhere has to be a
/// place two concurrent test binaries cannot collide in — which is why the name
/// carries the process id and a counter as well as whatever the caller called
/// it.
///
/// **It is not created here.** [`App::capture`](corvid_app::App::capture) is
/// what creates a capture directory, and handing it one that already exists
/// would not be exercising that. Anything already at the path is removed, so a
/// process id that has come round again does not inherit a previous run's
/// frames.
///
/// ```
/// let scratchpad = corvid_test::Scratchpad::new("example");
/// let path = scratchpad.path().to_path_buf();
/// assert!(!path.exists());
///
/// std::fs::create_dir_all(&path).unwrap();
/// assert!(path.exists());
///
/// drop(scratchpad);
/// assert!(!path.exists());
/// ```
#[derive(Debug)]
pub struct Scratchpad {
    /// Where it is.
    path: PathBuf,
}

impl Scratchpad {
    /// A directory nothing else is using, named for whoever asked.
    #[must_use]
    pub fn new(what: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "corvid_test-{}-{what}-{unique}",
            std::process::id()
        ));
        drop(fs::remove_dir_all(&path));
        Self { path }
    }

    /// Where it is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratchpad {
    /// Removes the directory, and says nothing if it cannot.
    ///
    /// A drop cannot report and this crate cannot panic, so a temporary
    /// directory that outlives its owner is a few kilobytes in the system's
    /// temporary directory rather than a failure in whatever was running.
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.path));
    }
}
