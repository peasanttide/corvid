#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

//! The one source that opens a directory, and the listing a level walks it with.

use std::fs;

use corvid_asset::{Files, Source};

/// A directory laid out under the process's own target directory.
///
/// No `tempfile`: this crate has no dev-dependency on one, and a fixed path
/// under `target` is removed and rebuilt at the top of each test, so a rerun
/// after a crash starts clean rather than inheriting whatever was left.
fn scratch(name: &str) -> std::path::PathBuf {
    let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    drop(fs::remove_dir_all(&root));
    fs::create_dir_all(root.join("props")).expect("a directory under the target directory");
    root
}

#[test]
fn listing_a_directory_answers_paths_that_read_back() {
    let root = scratch("listing");
    fs::write(root.join("props").join("bench.bin"), [1]).expect("a writable scratch");
    fs::write(root.join("props").join("lamp.bin"), [2]).expect("a writable scratch");

    let files = Files::new(&root);
    let found = files.list("props").expect("a directory that is there");

    assert_eq!(found, ["props/bench.bin", "props/lamp.bin"]);
    // The whole point of carrying the prefix: what `list` answered goes
    // straight back into `read` without a caller reassembling anything.
    for path in &found {
        assert!(files.read(path).is_ok(), "{path} did not read back");
    }
}

/// A trailing slash and no trailing slash name the same directory.
#[test]
fn a_trailing_slash_makes_no_difference() {
    let root = scratch("slash");
    fs::write(root.join("props").join("bench.bin"), [1]).expect("a writable scratch");

    let files = Files::new(&root);
    assert_eq!(
        files.list("props").expect("there"),
        files.list("props/").expect("there"),
    );
}

/// A level with no props is a level.
#[test]
fn a_directory_that_is_not_there_lists_nothing_rather_than_failing() {
    let files = Files::new(scratch("absent"));
    assert_eq!(
        files
            .list("nowhere")
            .expect("an absent directory is not a failure"),
        Vec::<String>::new(),
    );
}

/// The listing is sorted, whatever order the filesystem stored it in.
///
/// Load-bearing rather than tidy: a level built out of whatever `read_dir`
/// answered would have contents that depend on a directory's internal order,
/// and two peers that walked their props differently hash two different levels.
#[test]
fn listing_is_sorted_however_the_directory_stored_it() {
    let root = scratch("sorted");
    for name in ["zebra", "alpha", "mid"] {
        fs::write(root.join("props").join(name), [0]).expect("a writable scratch");
    }
    let files = Files::new(&root);
    assert_eq!(
        files.list("props").expect("three entries"),
        ["props/alpha", "props/mid", "props/zebra"],
    );
}

/// The escape refusal `read` has, `list` has too.
///
/// A level name that arrived over a network must not be able to enumerate a
/// directory outside the root any more than it can read a file outside it.
#[test]
fn listing_refuses_to_leave_the_root() {
    let files = Files::new(scratch("escape"));
    assert!(files.list("../..").is_err());
    assert!(files.read("../../etc/passwd").is_err());
}
