//! What a latest-value cell promises, one property per test.
//!
//! This is the semantics: what a fresh [`Seen`] reports, what a poll says has
//! changed, what a `modify` publishes, and what the two handles print. The
//! properties about a thread not waiting are in `tests/blocking.rs`, the ones
//! about what runs outside the lock in `tests/lock.rs`, the contention in
//! `tests/threads.rs`, and the spans in `tests/tracing.rs`.

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use std::panic::{self, AssertUnwindSafe};

use common::within;
use corvid_signal::{Seen, channel};

#[test]
fn a_fresh_seen_reports_the_value_that_was_already_there() {
    let (_emit, watch) = channel("surface", 1280_u32);
    let mut seen = Seen::default();

    assert_eq!(watch.changed_since(&mut seen).as_deref(), Some(&1280));
    assert_eq!(watch.changed_since(&mut seen).as_deref(), None);
}

#[test]
fn seen_now_starts_from_the_value_in_the_cell() {
    let (emit, watch) = channel("surface", 1280_u32);
    let mut seen = watch.seen_now();

    // The opposite starting point to the test above: this consumer has read the
    // state already and is asking only for what happens next.
    assert_eq!(watch.changed_since(&mut seen).as_deref(), None);
    emit.set(1920);
    assert_eq!(watch.changed_since(&mut seen).as_deref(), Some(&1920));
}

/// A `Seen` is eight bytes and holds no channel identity, which the README and
/// `Seen`'s own page both say twice.
///
/// The number is here because the thing it rules out is a plausible improvement
/// rather than a mistake: the documented footgun is polling a `Seen` against
/// the wrong `Watch`, and the obvious cure is to carry something that says
/// which signal it came from. Doing that is a decision about the array of
/// `Seen`s a consumer keeps beside its array of watches, not a detail, so it
/// should fail here rather than quietly widen the type.
#[test]
fn a_seen_is_a_sequence_number_and_nothing_else() {
    assert_eq!(size_of::<Seen>(), 8);
}

#[test]
fn a_watcher_sees_only_the_latest_of_three_publications() {
    let (emit, watch) = channel("peers", 0_u32);
    let mut seen = watch.seen_now();

    emit.set(1);
    emit.set(2);
    emit.set(3);

    // Not `Some(1)`, which is what a queue would say, and not three `Some`s.
    assert_eq!(watch.changed_since(&mut seen).as_deref(), Some(&3));
    assert_eq!(watch.changed_since(&mut seen).as_deref(), None);
}

#[test]
fn changed_since_reports_a_change_exactly_once() {
    let (emit, watch) = channel("peers", 0_u32);
    let mut seen = watch.seen_now();

    for expected in 1..=8_u32 {
        assert_eq!(
            watch.changed_since(&mut seen).as_deref(),
            None,
            "before publishing"
        );
        emit.set(expected);
        assert_eq!(watch.changed_since(&mut seen).as_deref(), Some(&expected));
        assert_eq!(
            watch.changed_since(&mut seen).as_deref(),
            None,
            "after observing"
        );
    }
}

#[test]
fn two_watchers_keep_their_own_place() {
    let (emit, watch) = channel("peers", 0_u32);
    let mut eager = watch.seen_now();
    let mut idle = watch.seen_now();

    emit.set(1);
    assert_eq!(watch.changed_since(&mut eager).as_deref(), Some(&1));
    emit.set(2);
    assert_eq!(watch.changed_since(&mut eager).as_deref(), Some(&2));

    // The consumer that did not look sees the latest, once, and is then level
    // with the one that looked twice.
    assert_eq!(watch.changed_since(&mut idle).as_deref(), Some(&2));
    assert_eq!(watch.changed_since(&mut idle).as_deref(), None);
    assert_eq!(watch.changed_since(&mut eager).as_deref(), None);
}

#[test]
fn every_clone_of_a_handle_is_a_handle_on_the_same_cell() {
    let (emit, watch) = channel("peers", 0_u32);
    let second_emitter = emit.clone();
    let second_watch = watch.clone();
    let mut seen = watch.seen_now();

    emit.set(4);
    second_emitter.set(5);
    assert_eq!(*watch.get(), 5);
    assert_eq!(second_watch.changed_since(&mut seen).as_deref(), Some(&5));
}

#[test]
fn modify_edits_in_place_and_publishes_the_result() {
    let (emit, watch) = channel("audio devices", vec!["built-in"]);
    let mut seen = watch.seen_now();

    emit.modify(|devices| devices.push("headset"));

    assert_eq!(
        watch.changed_since(&mut seen).as_deref(),
        Some(&vec!["built-in", "headset"])
    );
    assert_eq!(watch.changed_since(&mut seen).as_deref(), None);
}

#[test]
fn modify_publishes_even_when_it_changed_nothing() {
    let (emit, watch) = channel("audio devices", vec!["built-in"]);
    let mut seen = watch.seen_now();

    emit.modify(|_| {});

    // Documented rather than incidental: `T` is not required to be `PartialEq`,
    // so nothing here can tell an edit from a no-op, and a signal that only
    // woke consumers for changes it could prove were changes would behave
    // differently for a game that implemented `PartialEq` than for one that did
    // not.
    assert_eq!(
        watch.changed_since(&mut seen).as_deref(),
        Some(&vec!["built-in"])
    );
}

#[test]
fn a_modify_that_panics_leaves_its_edit_and_publishes_nothing() {
    let (emit, watch) = channel("audio devices", vec!["built-in"]);
    let mut seen = watch.seen_now();

    let escaped = panic::catch_unwind(AssertUnwindSafe(|| {
        emit.modify(|devices| {
            devices.push("headset");
            panic!("the closure gave up half way");
        });
    }));
    assert!(escaped.is_err(), "the panic should have escaped `modify`");

    // Everything below is documented on `modify`, and is here because a
    // sentence about what a panic leaves behind is a claim like any other. The
    // edit is in the cell, because `f` writes through the value itself.
    assert_eq!(*watch.get(), vec!["built-in", "headset"]);
    // And no consumer is told, because the sequence number is bumped after `f`
    // returns and this `f` did not return.
    assert_eq!(watch.changed_since(&mut seen).as_deref(), None);
    // And the signal keeps working: the lock is poisoned by that panic, and
    // this crate serves the value as it stands rather than refusing to.
    emit.set(vec!["built-in"]);
    assert_eq!(
        watch.changed_since(&mut seen).as_deref(),
        Some(&vec!["built-in"])
    );
}

/// Both handles hand back the label, and print it without reading the value.
///
/// The `Debug` half is not decoration. Both implementations name the signal and
/// deliberately stop there, because reading the value means taking the lock and
/// the likeliest place a handle gets formatted is a `modify` closure that is
/// already holding it -- so a `Debug` that reached for the value would turn a
/// log line into a deadlock. Formatting both handles from inside one is the
/// only way to assert that rather than describe it, and the backstop is what
/// turns the deadlock it is looking for into a message instead of a hang.
#[test]
fn both_handles_name_the_signal_and_print_nothing_of_the_value() {
    let (emit, watch) = channel("audio devices", vec!["built-in"]);
    assert_eq!(emit.label(), "audio devices");
    assert_eq!(watch.label(), "audio devices");

    let printed = within(
        "formatting both handles inside a `modify` closure",
        move || {
            let mut printed = Vec::new();
            emit.modify(|_| {
                printed.push(format!("{emit:?}"));
                printed.push(format!("{watch:?}"));
            });
            printed
        },
    );

    assert!(printed[0].starts_with("Emitter"), "{printed:?}");
    assert!(printed[1].starts_with("Watch"), "{printed:?}");
    for line in &printed {
        assert!(
            line.contains("audio devices"),
            "{line} does not name the signal"
        );
        assert!(!line.contains("built-in"), "{line} printed the value");
    }
}

#[test]
fn a_snapshot_does_not_change_underneath_its_reader() {
    let (emit, watch) = channel("audio devices", vec!["built-in"]);
    let snapshot = watch.get();

    emit.modify(|devices| devices.push("headset"));
    emit.set(vec!["nothing at all"]);

    // The copy-on-write step is what holds this: an edit through `modify` goes
    // to a copy while a consumer is reading the original, so a value handed out
    // stays the value it was handed out as for as long as it is held.
    assert_eq!(*snapshot, vec!["built-in"]);
    assert_eq!(*watch.get(), vec!["nothing at all"]);
}
