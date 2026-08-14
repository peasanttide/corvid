//! What the golden comparison notices, what it deliberately does not, and what
//! blessing writes.
//!
//! # The environment, and how the blessing half is reached without touching it
//!
//! Blessing is selected by an environment variable, a process has one
//! environment, and `std::env::set_var` is `unsafe` -- which this workspace
//! forbids rather than merely denies. So the test that blesses runs *itself* in
//! a child process with the variable set and one test named, which reaches the
//! real `matches_goldens` through the real environment rather than through a
//! back door, and leaves every other test in this binary running in an
//! environment nobody moved.
//!
//! The consequence, worth stating because somebody will hit it: these tests
//! assume `CORVID_BLESS` is **not** set in the environment they are run in. It
//! is a per-package flag -- `CORVID_BLESS=1 cargo test -p headless` -- and running
//! a whole workspace under it would ask this file's comparisons to bless the
//! fixtures they are asserting about.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

use std::{fs, path::Path, process::Command};

use corvid_test::{BLESS, Finding, How, Mismatch, Scratchpad, hex, matches_goldens, unhex};

/// Writes `contents` at `path`, creating the directories above it.
fn put(path: &Path, contents: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A capture directory holding two files, one of them in a subdirectory.
fn capture(scratchpad: &Scratchpad) -> &Path {
    put(&scratchpad.path().join("trace"), &[1, 2, 3, 4]);
    put(&scratchpad.path().join("draw").join("42"), &[0xab, 0xcd]);
    scratchpad.path()
}

/// A goldens directory holding what [`capture`] writes.
fn goldens(scratchpad: &Scratchpad) -> &Path {
    put(
        &scratchpad.path().join("trace.hex"),
        hex(&[1, 2, 3, 4]).as_bytes(),
    );
    put(
        &scratchpad.path().join("draw").join("42.hex"),
        hex(&[0xab, 0xcd]).as_bytes(),
    );
    scratchpad.path()
}

/// Whether this process is the child the blessing test spawned.
fn blessing() -> bool {
    std::env::var_os(BLESS).is_some_and(|value| !value.is_empty())
}

#[test]
fn a_capture_that_matches_its_goldens_passes() {
    let (one, two) = (
        Scratchpad::new("match-capture"),
        Scratchpad::new("match-goldens"),
    );
    matches_goldens(capture(&one), goldens(&two)).unwrap();
}

#[test]
fn one_byte_that_moved_names_the_file_and_the_offset_and_leaves_the_other_alone() {
    // The test above passes for a comparison that compares nothing. This is what
    // says it does not.
    let (one, two) = (
        Scratchpad::new("moved-capture"),
        Scratchpad::new("moved-goldens"),
    );
    let (capture, goldens) = (capture(&one), goldens(&two));
    put(&capture.join("trace"), &[1, 2, 0xff, 4]);

    let Err(mismatch) = matches_goldens(capture, goldens) else {
        panic!("the comparison agreed");
    };
    let Mismatch::Moved { ref findings, .. } = mismatch else {
        panic!("{mismatch}");
    };
    assert_eq!(
        findings,
        &vec![Finding {
            what: "trace".to_owned(),
            how: How::Moved {
                at: 2,
                recorded: 4,
                captured: 4,
            },
        }],
    );
    // And it counts in English. A report that reads like a bug is one a person
    // distrusts at the moment they most need to trust it.
    let message = mismatch.to_string();
    assert!(message.contains("1 golden in"), "{message}");
    assert!(message.contains("no longer says"), "{message}");
}

#[test]
fn a_capture_that_is_a_prefix_of_its_golden_reports_the_length_it_stopped_at() {
    // The other half of `How::Moved`, and the shape a truncated write takes: no
    // byte disagrees, there are simply fewer of them. `at` is the shorter length
    // rather than a disagreement, because there is no offset where the two
    // differ -- a comparison that reported "offset 4" here would be naming a byte
    // the capture does not have.
    let (one, two) = (
        Scratchpad::new("prefix-capture"),
        Scratchpad::new("prefix-goldens"),
    );
    let (capture, goldens) = (capture(&one), goldens(&two));
    put(&capture.join("trace"), &[1, 2, 3]);

    let Err(mismatch) = matches_goldens(capture, goldens) else {
        panic!("the comparison agreed");
    };
    let Mismatch::Moved { ref findings, .. } = mismatch else {
        panic!("{mismatch}");
    };
    assert_eq!(
        findings,
        &vec![Finding {
            what: "trace".to_owned(),
            how: How::Moved {
                at: 3,
                recorded: 4,
                captured: 3,
            },
        }],
    );
    // And the message says both lengths. "first differing at offset 3" on its
    // own would send a reader looking for a byte that moved, when what moved is
    // how many there are.
    let message = mismatch.to_string();
    assert!(
        message.contains("4 bytes recorded against 3 captured"),
        "{message}",
    );
    assert!(message.contains("first differing at offset 3"), "{message}");
}

#[test]
fn a_capture_longer_than_its_golden_reports_the_same_way_round() {
    // The direction the other way, because the two lengths are named in a fixed
    // order and a comparison that printed whichever was larger first would
    // report a grown capture as a shrunken one.
    let (one, two) = (
        Scratchpad::new("longer-capture"),
        Scratchpad::new("longer-goldens"),
    );
    let (capture, goldens) = (capture(&one), goldens(&two));
    put(&capture.join("trace"), &[1, 2, 3, 4, 5, 6]);

    let Err(mismatch) = matches_goldens(capture, goldens) else {
        panic!("the comparison agreed");
    };
    let Mismatch::Moved { ref findings, .. } = mismatch else {
        panic!("{mismatch}");
    };
    assert_eq!(
        findings,
        &vec![Finding {
            what: "trace".to_owned(),
            how: How::Moved {
                at: 4,
                recorded: 4,
                captured: 6,
            },
        }],
    );
    let message = mismatch.to_string();
    assert!(
        message.contains("4 bytes recorded against 6 captured"),
        "{message}",
    );
}

#[test]
fn every_golden_that_moved_is_named_at_once() {
    // A deliberate format change moves every golden, and a report that stopped
    // at the first would show a deliberate change and an accidental one
    // identically.
    let (one, two) = (
        Scratchpad::new("all-capture"),
        Scratchpad::new("all-goldens"),
    );
    let (capture, goldens) = (capture(&one), goldens(&two));
    put(&capture.join("trace"), &[9, 9, 9, 9]);
    put(&capture.join("draw").join("42"), &[9, 9]);

    let Err(mismatch) = matches_goldens(capture, goldens) else {
        panic!("the comparison agreed");
    };
    let Mismatch::Moved { ref findings, .. } = mismatch else {
        panic!("{mismatch}");
    };
    // Sorted, so that a report of the same difference reads the same way twice.
    assert_eq!(
        findings.iter().map(|f| f.what.as_str()).collect::<Vec<_>>(),
        ["draw/42", "trace"],
    );

    let message = mismatch.to_string();
    assert!(message.contains("2 goldens in"), "{message}");
    assert!(message.contains("no longer say what"), "{message}");
    assert!(message.contains("draw/42"), "{message}");
    assert!(message.contains("trace"), "{message}");
    assert!(message.contains("offset 0"), "{message}");
    assert!(message.contains(BLESS), "{message}");
}

#[test]
fn a_golden_the_capture_has_no_file_for_is_a_finding() {
    // A run that stopped producing a file somebody froze. Blessing cannot fix
    // this one, because there is nothing to record it from.
    let (one, two) = (
        Scratchpad::new("gone-capture"),
        Scratchpad::new("gone-goldens"),
    );
    let (capture, goldens) = (capture(&one), goldens(&two));
    fs::remove_file(capture.join("draw").join("42")).unwrap();

    let Err(Mismatch::Moved { findings, .. }) = matches_goldens(capture, goldens) else {
        panic!("the comparison agreed");
    };
    assert_eq!(
        findings,
        vec![Finding {
            what: "draw/42".to_owned(),
            how: How::Absent,
        }],
    );
}

#[test]
fn a_capture_file_with_no_golden_beside_it_is_not_frozen() {
    // The other direction, and the design: a capture holds two files per tick
    // and nobody freezes two hundred of them, so the goldens directory is the
    // frozen set and a file with no golden is not compared. What makes that safe
    // rather than lax is the test above -- a golden that names a file the capture
    // has stopped holding still fails.
    let (one, two) = (
        Scratchpad::new("extra-capture"),
        Scratchpad::new("extra-goldens"),
    );
    let (capture, goldens) = (capture(&one), goldens(&two));
    put(&capture.join("draw").join("43"), &[0, 0, 0]);

    matches_goldens(capture, goldens).unwrap();
}

#[test]
fn a_goldens_directory_with_nothing_in_it_is_never_a_pass() {
    // The failure mode a golden test exists to avoid: a comparison with nothing
    // in it, going green forever.
    let (one, two) = (
        Scratchpad::new("unfrozen-capture"),
        Scratchpad::new("unfrozen-goldens"),
    );
    let capture = capture(&one);

    // Absent entirely, which is what a game looks like before it has frozen
    // anything.
    assert!(matches!(
        matches_goldens(capture, two.path()),
        Err(Mismatch::Unfrozen { .. }),
    ));

    // And present with nothing in it that is a golden. A README beside the
    // goldens is not a golden.
    put(&two.path().join("README.md"), b"these are goldens");
    assert!(matches!(
        matches_goldens(capture, two.path()),
        Err(Mismatch::Unfrozen { .. }),
    ));
}

#[test]
fn a_golden_that_is_not_hex_is_a_finding_rather_than_a_pass() {
    // A golden somebody edited by hand into something that is not bytes. It has
    // to fail: `unhex` answering `None` and the comparison reading that as
    // "nothing recorded, nothing to disagree with" is exactly how a golden test
    // goes quiet.
    let (one, two) = (
        Scratchpad::new("junk-capture"),
        Scratchpad::new("junk-goldens"),
    );
    let (capture, goldens) = (capture(&one), goldens(&two));
    put(&goldens.join("trace.hex"), b"not bytes\n");

    let Err(Mismatch::Moved { findings, .. }) = matches_goldens(capture, goldens) else {
        panic!("the comparison agreed");
    };
    assert_eq!(
        findings,
        vec![Finding {
            what: "trace".to_owned(),
            how: How::Malformed,
        }],
    );
}

#[test]
fn blessing_records_the_capture_and_still_fails() {
    if !blessing() {
        // The parent half. Run this one test again with the variable set,
        // through the real environment, and require the child to pass.
        let status = Command::new(std::env::current_exe().unwrap())
            .env(BLESS, "1")
            .args([
                "blessing_records_the_capture_and_still_fails",
                "--exact",
                "--nocapture",
            ])
            .status()
            .unwrap();
        assert!(status.success(), "the blessing half failed; see above");
        return;
    }

    let (one, two) = (
        Scratchpad::new("bless-capture"),
        Scratchpad::new("bless-goldens"),
    );
    let (moved, recorded) = (capture(&one), goldens(&two));
    put(&moved.join("trace"), &[9, 9, 9, 9]);

    // It fails, and that is the design: a blessing run that went green would
    // tell nobody what it had changed, and a job with the variable set by
    // accident would go green forever.
    let Err(Mismatch::Rewritten { ref findings, .. }) = matches_goldens(moved, recorded) else {
        panic!("blessing did not report what it rewrote");
    };
    assert_eq!(
        findings,
        &vec![Finding {
            what: "trace".to_owned(),
            how: How::Moved {
                at: 0,
                recorded: 4,
                captured: 4,
            },
        }],
    );

    // What it wrote is what the capture holds, in the form a golden is written
    // in -- so the next run passes, and a person can read the diff.
    assert_eq!(
        fs::read_to_string(recorded.join("trace.hex")).unwrap(),
        hex(&[9, 9, 9, 9]),
    );
    matches_goldens(moved, recorded).unwrap();

    // And a tree that already agrees is rewritten not at all, so blessing is a
    // diff rather than a count of files written.
    let (three, four) = (
        Scratchpad::new("clean-capture"),
        Scratchpad::new("clean-goldens"),
    );
    let (clean, frozen) = (capture(&three), goldens(&four));
    matches_goldens(clean, frozen).unwrap();

    // The one difference blessing cannot record, under blessing: there is
    // nothing to record it from, so it is still a finding.
    fs::remove_file(clean.join("draw").join("42")).unwrap();
    let Err(Mismatch::Rewritten { findings, .. }) = matches_goldens(clean, frozen) else {
        panic!("a golden with no capture file passed under blessing");
    };
    assert_eq!(findings[0].how, How::Absent);
}

#[test]
fn hex_survives_the_round_trip_and_rejects_what_is_not_hex() {
    let bytes: Vec<u8> = (0..=255_u8).collect();
    assert_eq!(unhex(&hex(&bytes)), Some(bytes));

    // The wrapping a golden is written with is not part of it.
    assert_eq!(unhex("00 ff\n01\n"), Some(vec![0x00, 0xff, 0x01]));
    // Half a byte is not a byte.
    assert_eq!(unhex("abc"), None);
    // And neither is something that is not a digit.
    assert_eq!(unhex("zz"), None);
}

#[test]
fn a_golden_is_wrapped_where_a_person_can_read_it() {
    // Thirty-two bytes to the line, so a diff that says one line changed is a
    // diff about roughly one field rather than about a whole capture.
    let text = hex(&(0..96_u8).collect::<Vec<u8>>());
    assert_eq!(text.lines().count(), 3);
    assert!(text.lines().all(|line| line.len() == 64), "{text}");
}
