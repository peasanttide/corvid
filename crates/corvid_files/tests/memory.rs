#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

//! The one [`Source`] this crate ships, and the contract every other one owes.

use corvid_files::{Malformed, Memory, Missing, Source};

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

/// The two failures are different findings and the types keep them apart.
#[test]
fn absent_and_unreadable_are_not_the_same_failure() {
    let missing = Missing::new("level/court.bin");
    let malformed = Malformed::from(missing);
    assert_eq!(malformed.path.as_deref(), Some("level/court.bin"));
    assert_ne!(
        Malformed::at("level/court.bin", "the header is the wrong version"),
        malformed,
        "a file that is absent and a file that will not parse are two findings",
    );
}

/// A decoder that was handed bytes and nothing else says so, and whoever read
/// them fills the path in afterwards.
///
/// This is the arrangement that lets `Asset::decode` and `Level::load` raise one
/// type: the decoder knows what was wrong, the store knows where it came from,
/// and neither has to know the other's half.
#[test]
fn a_path_can_be_named_after_the_fact() {
    let from_a_decoder = Malformed::new("the header is the wrong version");
    assert_eq!(from_a_decoder.path, None);
    assert_eq!(
        from_a_decoder.in_file("level/court.bin"),
        Malformed::at("level/court.bin", "the header is the wrong version"),
    );
}
