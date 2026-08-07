#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

//! Load, share, evict, placeholder, LOD promotion.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use corvid_asset::{Asset, Assets, Lod, Malformed, Memory, Missing, Unavailable};

use corvid_asset::Source;
/// A source that says how often it was read.
#[derive(Debug)]
struct Counted {
    files: Memory,
    reads: Arc<AtomicUsize>,
}

impl Source for Counted {
    fn read(&self, path: &str) -> Result<Vec<u8>, Missing> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.files.read(path)
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, Missing> {
        self.files.list(prefix)
    }

    fn exists(&self, path: &str) -> bool {
        self.files.exists(path)
    }
}

/// One level, and the placeholder that is plainly not it.
#[derive(Debug, PartialEq)]
struct Note(String);

impl Asset for Note {
    fn placeholder() -> Self {
        Self("…".to_owned())
    }

    fn decode(bytes: &[u8], _lod: Lod) -> Result<Self, Malformed> {
        String::from_utf8(bytes.to_vec())
            .map(Self)
            .map_err(|_| Malformed::new("not utf-8"))
    }
}

/// Three levels, each of which says which one it is.
#[derive(Debug, PartialEq)]
struct Tiered {
    at: Lod,
    words: usize,
}

impl Asset for Tiered {
    fn placeholder() -> Self {
        Self {
            at: Lod::PLACEHOLDER,
            words: 0,
        }
    }

    fn decode(bytes: &[u8], lod: Lod) -> Result<Self, Malformed> {
        let text = str::from_utf8(bytes).map_err(|_| Malformed::new("not utf-8"))?;
        Ok(Self {
            at: lod,
            words: text.split_whitespace().count(),
        })
    }

    fn levels() -> u8 {
        3
    }
}

/// The registry, and a way to read the source's tally.
fn registry(files: Memory) -> (Assets, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let source = Counted {
        files,
        reads: Arc::clone(&reads),
    };
    (Assets::new(Box::new(source)), reads)
}

/// Poll until nothing is outstanding, or give up rather than hang.
fn settle(assets: &Assets) {
    for _ in 0..100_000 {
        assets.poll();
        if assets.is_settled() {
            return;
        }
        std::thread::yield_now();
    }
    assert!(assets.is_settled(), "the loader never settled");
}

fn one(path: &str, bytes: &[u8]) -> Memory {
    let mut files = Memory::new();
    files.insert(path, bytes.to_vec());
    files
}

#[test]
fn two_requests_for_one_path_read_the_source_once() {
    let (assets, reads) = registry(one("a", b"once"));

    let first = assets.load::<Note>("a");
    let second = assets.load::<Note>("a");
    settle(&assets);

    assert_eq!(reads.load(Ordering::Relaxed), 1);
    assert_eq!(first.get().0, "once");
    assert_eq!(second.get().0, "once");
    // The two handles and the cache's own reference.
    assert_eq!(first.holders(), 3);
}

#[test]
fn a_handle_answers_the_placeholder_until_the_load_lands() {
    let (assets, _) = registry(one("a", b"the real thing"));

    let note = assets.load::<Note>("a");
    assert!(!note.is_resident());
    assert_eq!(note.lod(), Lod::PLACEHOLDER);
    assert_eq!(note.get().0, "…");

    let mut flips = 0;
    let mut was = note.is_resident();
    for _ in 0..100_000 {
        assets.poll();
        let now = note.is_resident();
        if now != was {
            flips += 1;
            was = now;
        }
        if assets.is_settled() {
            break;
        }
        std::thread::yield_now();
    }

    assert_eq!(flips, 1);
    assert!(note.is_resident());
    assert_eq!(note.lod(), Lod::FINEST);
    assert_eq!(note.get().0, "the real thing");
}

#[test]
fn evicting_frees_what_nothing_holds() {
    let (assets, _) = registry(one("a", b"four"));

    let note = assets.load_now::<Note>("a").expect("the path is there");
    let weak = note.downgrade();

    // A live handle keeps it, and eviction takes nothing.
    assert_eq!(assets.evict(), corvid_asset::Evicted::default());
    assert!(weak.upgrade().is_some());

    drop(note);
    let evicted = assets.evict();
    assert_eq!(evicted.assets, 1);
    assert_eq!(evicted.bytes, 4);
    assert!(weak.upgrade().is_none());
    assert!(assets.is_empty());
}

#[test]
fn a_missing_path_fails_the_handle_rather_than_panicking() {
    let (assets, _) = registry(Memory::new());

    let note = assets.load::<Note>("nowhere");
    settle(&assets);

    assert!(note.is_failed());
    assert!(!note.is_resident());
    // It still answers, so a renderer draws the placeholder rather than stops.
    assert_eq!(note.get().0, "…");

    let progress = assets.progress();
    assert_eq!(progress.requested, 1);
    assert_eq!(progress.failed, 1);
    assert_eq!(progress.resident, 0);
}

#[test]
fn a_malformed_asset_is_refused_by_name() {
    let (assets, _) = registry(one("a", &[0xff, 0xfe]));

    let refused = assets.load_now::<Note>("a").unwrap_err();
    assert_eq!(refused, Unavailable::Malformed(Malformed::new("not utf-8")));
    assert_eq!(assets.progress().failed, 1);
}

#[test]
fn a_failed_request_is_tried_again_and_counted_once() {
    let (assets, reads) = registry(one("a", &[0xff, 0xfe]));

    assert!(assets.load_now::<Note>("a").is_err());
    let again = assets.load_now::<Note>("a").unwrap_err();

    // The reason is this attempt's, not a remembered one.
    assert_eq!(again, Unavailable::Malformed(Malformed::new("not utf-8")));
    assert_eq!(reads.load(Ordering::Relaxed), 2);
    // And the barrier that already lifted stays lifted.
    assert_eq!(assets.progress().failed, 1);
    assert!(assets.is_settled());
}

#[test]
fn three_levels_promote_one_at_a_time() {
    let (assets, _) = registry(one("a", b"two words"));

    let tiered = assets.load::<Tiered>("a");
    assert_eq!(tiered.lod(), Lod::PLACEHOLDER);

    // The first poll that finds the decode installs the coarsest level.
    for _ in 0..100_000 {
        assets.poll();
        if tiered.is_resident() {
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(tiered.lod(), Lod(2));
    assert_eq!(tiered.get().at, Lod(2));

    assets.poll();
    assert_eq!(tiered.lod(), Lod(1));

    assets.poll();
    assert_eq!(tiered.lod(), Lod::FINEST);
    assert_eq!(tiered.get().words, 2);
    assert!(assets.is_settled());
}

#[test]
fn a_level_loads_with_no_file_on_disk() {
    let mut files = Memory::new();
    files.insert("levels/terminus", b"hall\ncellar".to_vec());

    let assets = Assets::new(Box::new(files));
    let level = assets
        .load_now::<Note>("levels/terminus")
        .expect("it is in memory");

    assert_eq!(level.get().0, "hall\ncellar");
}

#[test]
fn nothing_is_settled_until_the_last_request_is_drained() {
    let (assets, _) = registry(one("a", b"one"));

    assert!(assets.is_settled());
    let note = assets.load::<Note>("a");
    assert!(!assets.is_settled());

    settle(&assets);
    assert!(assets.is_settled());
    assert_eq!(assets.progress().resident, 1);
    assert!(note.is_resident());
}

#[test]
fn a_source_refuses_to_leave_its_root() {
    let files = corvid_asset::Files::new("assets");

    assert!(!files.exists("../secret"));
    assert!(files.read("/etc/passwd").is_err());
    assert!(files.read("").is_err());
}
