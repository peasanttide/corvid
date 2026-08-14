//! The containment, mechanised.
//!
//! This crate is the one in the workspace that relaxes `unsafe_code` from
//! `forbid` to `deny`, and `deny` is a weaker word: an `#[allow]` anywhere in
//! the crate could lift it, where `forbid` could not be lifted at all. These
//! tests are what replaces that guarantee, and they are honest about being
//! weaker -- they catch the word, not the intent.
//!
//! The second of them is the one that matters. "Nothing else in the workspace
//! may follow it" is a sentence in the spec; this is that sentence as a test.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    reason = "these tests build fixtures out of raw bit patterns and read matrices back as floats; every cast here is the thing under test rather than an oversight"
)]

use std::{
    fs,
    path::{Path, PathBuf},
};

/// The word. Spelled once, so the rest of this file talks about it rather than
/// containing it by accident.
const KEYWORD: &str = "unsafe";

/// The lint the manifests are read for.
const LINT: &str = "unsafe_code";

/// The one file in this crate allowed to hold the word.
const SEAM: &str = "runtime/vulkan.rs";

/// Whether `line` names the word itself, rather than a longer word that starts
/// with the same letters -- the lint's own name is the one that does.
fn holds(line: &str) -> bool {
    line.match_indices(KEYWORD).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + KEYWORD.len()..].chars().next();
        let word = |character: Option<char>| {
            character.is_some_and(|next| next == '_' || next.is_alphanumeric())
        };
        !word(before) && !word(after)
    })
}

/// Whether `line` is a comment and nothing else.
fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// This crate's directory.
fn here() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `.rs` file under `root`, in a fixed order.
fn sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut entries: Vec<_> = fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
            .map(|entry| entry.expect("a directory entry").path())
            .collect();
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                pending.push(entry);
            } else if entry.extension().is_some_and(|kind| kind == "rs") {
                found.push(entry);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn the_word_appears_in_one_file_of_this_crate_and_nowhere_else() {
    let src = here().join("src");
    let seam = src.join(SEAM);
    let mut scanned = 0;

    for path in sources(&src) {
        if path == seam {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap_or_else(|error| panic!("{error}"));
        // Read as text rather than parsed, so a *comment* holding the word is a
        // failure too. That is a false positive worth having: a comment about
        // this subject in a file that has nothing to do with it is a comment in
        // the wrong file.
        assert!(
            !source.contains(KEYWORD),
            "{} holds the word; {SEAM} is the only file that may",
            path.display()
        );
        scanned += 1;
    }

    assert!(
        scanned > 0,
        "the scan found no sources at all, which is not a pass"
    );
}

#[test]
fn no_other_crate_in_the_workspace_relaxes_the_lint() {
    let crates = here().parent().expect("crates/").to_path_buf();
    let mine = here().join("Cargo.toml");
    let mut checked = 0;

    let mut entries: Vec<_> = fs::read_dir(&crates)
        .unwrap_or_else(|error| panic!("{}: {error}", crates.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .collect();
    entries.sort();

    for entry in entries {
        let manifest = entry.join("Cargo.toml");
        if manifest == mine || !manifest.is_file() {
            continue;
        }
        let text = fs::read_to_string(&manifest).unwrap_or_else(|error| panic!("{error}"));
        // Declaring the lint is fine -- six crates spell the whole table out
        // rather than inheriting it. Declaring it and saying anything but
        // `forbid` is not, and that is the clause: this crate is the one
        // exception, and nothing else in the workspace may follow it.
        //
        // A *declaration* is the name followed by `=`, and the distinction is
        // load-bearing rather than pedantic: `corvid_glm`'s manifest explains
        // in a comment that it takes `bytemuck` rather than casting on its own
        // "because `unsafe_code` is forbidden workspace-wide", which is a
        // sentence agreeing with this rule. Matching the bare name reads that
        // agreement as a violation.
        let declared = format!("{LINT} = ");
        if text.contains(&declared) {
            assert!(
                text.contains(&format!("{LINT} = \"forbid\"")),
                "{} declares {LINT} without forbidding it; corvid_xr is the workspace's one \
                 exception",
                manifest.display()
            );
        }
        for relaxed in ["deny", "allow", "warn"] {
            assert!(
                !text.contains(&format!("{LINT} = \"{relaxed}\"")),
                "{} relaxes {LINT} to `{relaxed}`; corvid_xr is the workspace's one exception",
                manifest.display()
            );
        }
        checked += 1;
    }

    assert!(
        checked > 5,
        "only {checked} manifests were read, which is not a workspace"
    );
}

#[test]
fn this_crates_manifest_relaxes_the_lint_to_deny_and_nothing_further() {
    let text = fs::read_to_string(here().join("Cargo.toml")).expect("this crate's manifest");
    assert!(
        text.contains(&format!("{LINT} = \"deny\"")),
        "the exception is `deny`, which is what lets one file lift it"
    );
    assert!(
        !text.contains(&format!("{LINT} = \"allow\"")),
        "`allow` would lift it everywhere, which is the thing the seam exists to avoid"
    );
}

#[test]
fn the_workspace_itself_still_forbids_it() {
    let root = here()
        .parent()
        .and_then(Path::parent)
        .expect("the workspace root")
        .join("Cargo.toml");
    let text =
        fs::read_to_string(&root).unwrap_or_else(|error| panic!("{}: {error}", root.display()));
    assert!(
        text.contains(&format!("{LINT} = \"forbid\"")),
        "the workspace's own table is what this crate is the exception to"
    );
}

#[test]
fn every_block_in_the_seam_carries_a_safety_comment() {
    let seam = here().join("src").join(SEAM);
    let source =
        fs::read_to_string(&seam).unwrap_or_else(|error| panic!("{}: {error}", seam.display()));

    let lines: Vec<&str> = source.lines().collect();
    let mut blocks = 0;
    for (number, line) in lines.iter().enumerate() {
        // Comments and the file-level attribute mention the subject without
        // being it; everything else that names the word is a block, a function
        // or an implementation, and every one of those has to say why.
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("#!") || !holds(line) {
            continue;
        }
        // `rustfmt` wraps a statement across as many lines as it needs, so step
        // back over the rest of the statement before looking for the comment,
        // and then to the top of the comment block -- what has to say `SAFETY`
        // is its first line rather than its last.
        let mut top = number;
        while top > 0 && !lines[top - 1].trim().is_empty() && !is_comment(lines[top - 1]) {
            top -= 1;
        }
        let mut first = top;
        while first > 0 && is_comment(lines[first - 1]) {
            first -= 1;
        }
        assert!(
            first < top && lines[first].trim_start().starts_with("// SAFETY:"),
            "{}:{} has no safety comment above it",
            seam.display(),
            number + 1
        );
        blocks += 1;
    }

    assert!(
        blocks > 0,
        "the seam holds nothing, so there is nothing to contain"
    );
}
