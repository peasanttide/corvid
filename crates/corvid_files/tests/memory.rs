//! The one [`Source`] this crate ships with files in it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

use std::collections::BTreeMap;

use corvid_files::{Memory, Missing, Source};

#[test]
fn a_memory_source_reads_back_what_was_put_in_it() {
    let mut files = Memory::new();
    files.insert("level/court.bin", vec![1, 2, 3]);
    assert_eq!(
        files.read("level/court.bin").expect("just inserted"),
        [1, 2, 3]
    );
}

#[test]
fn a_path_that_is_not_there_says_which_path() {
    let files = Memory::new();
    let why = files
        .read("level/absent.bin")
        .expect_err("nothing was inserted");
    assert_eq!(why, Missing::new("level/absent.bin"));
}

/// Putting a file somewhere answers whatever was there, and taking it away
/// answers what went.
///
/// The return values are the whole difference between this and a map that
/// silently overwrites: a loader that replaced a file it did not mean to
/// replace is told so at the call rather than at the next read.
#[test]
fn putting_a_file_in_and_taking_it_out_answer_what_was_there() {
    let mut files = Memory::new();
    assert!(files.is_empty());
    assert_eq!(files.len(), 0);

    assert_eq!(files.insert("level/court.bin", vec![1]), None);
    assert_eq!(
        files.insert("level/court.bin", vec![2]),
        Some(vec![1]),
        "a path that was already occupied hands back its old bytes"
    );
    assert_eq!(files.len(), 1, "and does not become a second file");
    assert!(!files.is_empty());

    assert_eq!(files.remove("level/court.bin"), Some(vec![2]));
    assert_eq!(files.remove("level/court.bin"), None);
    assert!(files.is_empty());
}

/// `Memory` overrides `exists` to ask the map for a key instead of reading a
/// file and dropping the bytes, and an override that disagreed with the read it
/// stands in for would be worse than no override at all.
#[test]
fn asking_whether_a_file_is_there_agrees_with_reading_it() {
    let mut files = Memory::new();
    files.insert("level/court.bin", vec![1, 2, 3]);
    for path in ["level/court.bin", "level/absent.bin", "level/", ""] {
        assert_eq!(
            files.exists(path),
            files.read(path).is_ok(),
            "the two answers parted company on {path:?}"
        );
    }
}

#[test]
fn listing_answers_the_paths_under_a_prefix_and_nothing_else() {
    let mut files = Memory::new();
    files.insert("level/court.bin", vec![]);
    files.insert("level/mesh.bin", vec![]);
    files.insert("sound/knock.wav", vec![]);

    let found = files
        .list("level/")
        .expect("a prefix with entries under it");
    assert_eq!(found, ["level/court.bin", "level/mesh.bin"]);
}

/// A prefix with nothing under it is an answer, not a failure.
///
/// "There are no props in this level" is a thing a loader needs to be told, and
/// a level that has to name every file it might read is a level that cannot
/// grow one.
#[test]
fn an_empty_prefix_lists_nothing_rather_than_failing() {
    let mut files = Memory::new();
    files.insert("level/court.bin", vec![]);
    assert_eq!(
        files
            .list("props/")
            .expect("an empty prefix is not an error"),
        Vec::<String>::new()
    );
}

/// The listing is sorted, and that is load-bearing rather than incidental.
///
/// A level built out of whatever `list` answered would otherwise be a level
/// whose contents depend on a map's iteration order — and a peer that walked
/// its props in a different order is a peer that hashes a different level.
#[test]
fn listing_is_ordered_so_two_peers_walk_a_level_the_same_way() {
    let mut files = Memory::new();
    for name in ["c", "a", "b"] {
        files.insert(format!("props/{name}"), vec![]);
    }
    assert_eq!(
        files.list("props/").expect("three entries"),
        ["props/a", "props/b", "props/c"]
    );
}

/// A prefix that is a prefix of a *name* rather than of a directory still
/// matches by string, which is what "prefix" means here.
#[test]
fn a_prefix_is_a_string_prefix_and_says_so() {
    let mut files = Memory::new();
    files.insert("level/court.bin", vec![]);
    files.insert("level/courtyard.bin", vec![]);
    assert_eq!(
        files.list("level/court").expect("two entries share it"),
        ["level/court.bin", "level/courtyard.bin"]
    );
}

/// The empty prefix is every file, and the neighbour that sorts just past the
/// end of a prefix is not in it.
///
/// The listing walks a range and stops at the first key that no longer shares
/// the prefix, which is only correct because sorted order puts every sharing
/// key together and puts them all first. `level.` sorts before `level/` and
/// `level0` after it, so both sit either side of the window a `level/` prefix
/// opens and neither may appear in it.
#[test]
fn listing_stops_at_the_edges_of_the_prefix_and_not_before_or_after() {
    let mut files = Memory::new();
    for path in ["level.bin", "level/court.bin", "level0.bin"] {
        files.insert(path, vec![]);
    }

    assert_eq!(
        files.list("level/").expect("one entry is under it"),
        ["level/court.bin"]
    );
    assert_eq!(
        files.list("").expect("every file shares the empty prefix"),
        ["level.bin", "level/court.bin", "level0.bin"],
        "and the empty prefix is still sorted"
    );
}

/// The two bulk constructors agree with inserting the same files one at a time.
///
/// Both exist so that a game with its levels compiled in does not have to write
/// a `mut` binding and a run of `insert` calls to hand `load` something.
#[test]
fn collecting_and_converting_build_what_inserting_would_have() {
    let mut inserted = Memory::new();
    inserted.insert("level/court.bin", vec![1]);
    inserted.insert("level/mesh.bin", vec![2]);

    let collected: Memory = [("level/court.bin", vec![1]), ("level/mesh.bin", vec![2])]
        .into_iter()
        .collect();
    assert_eq!(collected, inserted);

    let converted = Memory::from(BTreeMap::from([
        (String::from("level/court.bin"), vec![1]),
        (String::from("level/mesh.bin"), vec![2]),
    ]));
    assert_eq!(converted, inserted);
}
