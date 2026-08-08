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

/// The listing is every path in the source and nothing else.
///
/// No prefix: a source is the files one level is read through rather than a
/// whole disk, so narrowing is the caller's `filter` and the trait owes one
/// rule instead of two.
#[test]
fn listing_answers_every_path_in_the_source() {
    let mut files = Memory::new();
    files.insert("level/court.bin", vec![]);
    files.insert("level/mesh.bin", vec![]);
    files.insert("sound/knock.wav", vec![]);

    assert_eq!(
        files.list().expect("a map is always askable"),
        ["level/court.bin", "level/mesh.bin", "sound/knock.wav"]
    );
}

/// A source with nothing in it is an answer, not a failure.
///
/// "There are no props in this level" is a thing a loader needs to be told, and
/// a level that has to name every file it might read is a level that cannot
/// grow one.
#[test]
fn an_empty_source_lists_nothing_rather_than_failing() {
    assert_eq!(
        Memory::new().list().expect("an empty map is not an error"),
        Vec::<String>::new()
    );
}

/// The listing is sorted, and that is load-bearing rather than incidental.
///
/// A level built out of whatever `list` answered would otherwise be a level
/// whose contents depend on a map's iteration order — and a peer that walked
/// its props in a different order is a peer that hashes a different level. The
/// paths go in unsorted on purpose, so an implementation that merely handed
/// back its own iteration order would fail here.
#[test]
fn listing_is_ordered_so_two_peers_walk_a_level_the_same_way() {
    let mut files = Memory::new();
    for name in ["c", "a", "b"] {
        files.insert(format!("props/{name}"), vec![]);
    }
    assert_eq!(
        files.list().expect("three entries"),
        ["props/a", "props/b", "props/c"]
    );
}

/// Sorted means sorted by byte, including across the separator, which is what a
/// caller filtering the listing itself is entitled to lean on.
///
/// `level.` sorts before `level/` and `level0` after it. A caller narrowing to
/// `level/` gets a contiguous run for exactly that reason, so the property the
/// old prefix walk relied on is still here — it has moved to the caller.
#[test]
fn the_order_is_by_byte_so_a_caller_can_narrow_by_prefix_itself() {
    let mut files = Memory::new();
    for path in ["level0.bin", "level/court.bin", "level.bin"] {
        files.insert(path, vec![]);
    }

    let all = files.list().expect("every file is in the listing");
    assert_eq!(all, ["level.bin", "level/court.bin", "level0.bin"]);

    let under: Vec<_> = all
        .iter()
        .filter(|path| path.starts_with("level/"))
        .collect();
    assert_eq!(under, ["level/court.bin"]);
}

/// `write` puts bytes where a read finds them, and replaces what was there.
///
/// The trait's method rather than `insert`: it copies from a borrow and answers
/// a `Result`, because the sources with a directory under them can refuse.
#[test]
fn writing_lands_where_reading_looks_and_replaces_what_was_there() {
    let mut files = Memory::new();
    files
        .write("level/court.bin", &[1, 2, 3])
        .expect("a map always has room");
    assert_eq!(
        files.read("level/court.bin").expect("just written"),
        [1, 2, 3]
    );

    files
        .write("level/court.bin", &[4])
        .expect("a map always has room");
    assert_eq!(
        files.read("level/court.bin").expect("just rewritten"),
        [4],
        "the second write replaced the first rather than appending to it"
    );
    assert_eq!(files.len(), 1, "and did not become a second file");
}

/// `write` and `insert` build the same map, differing only in what they take
/// and what they answer.
#[test]
fn writing_and_inserting_agree_about_what_the_map_holds() {
    let mut written = Memory::new();
    written
        .write("level/court.bin", &[1])
        .expect("a map always has room");

    let mut inserted = Memory::new();
    inserted.insert("level/court.bin", vec![1]);

    assert_eq!(written, inserted);
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
