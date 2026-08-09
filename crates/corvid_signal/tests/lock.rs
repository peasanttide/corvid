//! What the cell does outside the lock, which is everything a caller wrote.
//!
//! The lock is held for a pointer swap and a counter bump and for nothing else:
//! the value a publication replaced is dropped after it is released, a
//! consumer's clone of the value runs after it is released, and a `modify`
//! copies only when a consumer is holding the value it would otherwise edit in
//! place. Each of those is asserted here with a type that takes a visible
//! amount of time to drop or to clone, and with `common`'s backstops around
//! every wait.

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use common::{joined, once, within};
use corvid_signal::{Emitter, channel};

/// A value whose `Drop` publishes to the signal it was published on.
///
/// The shape this stands in for is a resource handle that reports its own
/// retirement -- the thing being dropped is what knows it is gone. It is the one
/// re-entrant path `set` is written to survive, by dropping the value it
/// replaced after releasing the lock rather than while holding it.
struct Retiring {
    /// Which value this is.
    tag: u64,
    /// `Some` on the value the test publishes, `None` on the one that value's
    /// `Drop` publishes, so the chain stops after one step.
    successor: Option<Emitter<Self>>,
}

impl Drop for Retiring {
    fn drop(&mut self) {
        if let Some(emitter) = self.successor.take() {
            emitter.set(Self {
                tag: self.tag + 1,
                successor: None,
            });
        }
    }
}

#[test]
fn the_value_a_set_replaced_is_dropped_outside_the_lock() {
    let (emit, watch) = channel(
        "retiring",
        Retiring {
            tag: 0,
            successor: None,
        },
    );

    emit.set(Retiring {
        tag: 1,
        successor: Some(emit.clone()),
    });

    within(
        "a set whose retired value publishes from its own Drop",
        move || {
            emit.set(Retiring {
                tag: 10,
                successor: None,
            });
        },
    );

    // Not 10: the value that was replaced published one of its own on the way
    // out, and that one landed after. Dropping it under the lock would have
    // deadlocked here instead, which is what the timeout above reports.
    assert_eq!(watch.get().tag, 2);
}

/// A `T` that reports how long it was cloned for, and how many threads were
/// inside `Watch::get` while it happened.
///
/// This is what turns "a publication never waits for a consumer" into an
/// assertion. Before the value moved behind an `Arc`, `T::clone` ran under the
/// same lock a publication takes, so a consumer holding this one for a whole
/// second held every publisher for a whole second with it.
#[derive(Debug)]
struct SlowClone {
    /// How long each clone of this value takes.
    delay: Duration,
    /// Bumped while a clone is in progress and dropped again after, so a
    /// publisher can say whether it landed during one.
    cloning: Arc<AtomicUsize>,
}

impl Clone for SlowClone {
    fn clone(&self) -> Self {
        self.cloning.fetch_add(1, Ordering::SeqCst);
        thread::sleep(self.delay);
        self.cloning.fetch_sub(1, Ordering::SeqCst);
        Self {
            delay: self.delay,
            cloning: Arc::clone(&self.cloning),
        }
    }
}

#[test]
fn a_consumer_taking_a_copy_does_not_hold_up_a_publisher() {
    let cloning = Arc::new(AtomicUsize::new(0));
    let value = SlowClone {
        // Long enough that a publication serialised behind it could not
        // possibly be mistaken for one that merely lost its timeslice.
        delay: Duration::from_millis(500),
        cloning: Arc::clone(&cloning),
    };
    let (emit, watch) = channel("slow", value);

    // A consumer reading the cell and then taking its own copy, which is the
    // shape a renderer reading a device list has. Both halves go on this
    // thread: `get` is the half this crate controls and the copy is the half
    // the consumer does, and the assertion below is that neither one is on the
    // publishing path.
    let copier = thread::spawn(move || {
        let snapshot = watch.get();
        let _copy = (*snapshot).clone();
    });

    // Wait until that copy is genuinely in progress rather than merely
    // scheduled, so this is a test about the lock and not about thread startup.
    once("a consumer's copy starting", || {
        cloning.load(Ordering::SeqCst) > 0
    });

    let took = within("a publication during a consumer's copy", move || {
        let started = Instant::now();
        emit.set(SlowClone {
            delay: Duration::from_millis(0),
            cloning: Arc::new(AtomicUsize::new(0)),
        });
        started.elapsed()
    });

    // The copy was still running when the publication went through, which is
    // what makes the figure below mean anything.
    assert!(
        took < Duration::from_millis(250),
        "a publication took {took:?} while a consumer was copying, so it queued behind the copy",
    );
    joined("the consumer taking a copy", copier);
}

/// A value that says how many times it has been cloned, and never stops
/// counting.
#[derive(Debug)]
struct Cloned(Arc<AtomicUsize>);

impl Clone for Cloned {
    fn clone(&self) -> Self {
        self.0.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(&self.0))
    }
}

#[test]
fn modify_copies_the_value_only_while_a_consumer_is_holding_it() {
    let clones = Arc::new(AtomicUsize::new(0));
    let (emit, watch) = channel("audio devices", Cloned(Arc::clone(&clones)));

    // Nobody is reading, so the edit lands where the value already is. This is
    // the case `modify` exists for, and it is still free.
    emit.modify(|_| {});
    assert_eq!(clones.load(Ordering::SeqCst), 0, "with nobody holding it");

    // A consumer holding a snapshot is what turns the edit into a copy: that
    // snapshot is a value somebody is reading and it may not change underneath
    // them.
    let snapshot = watch.get();
    emit.modify(|_| {});
    assert_eq!(
        clones.load(Ordering::SeqCst),
        1,
        "with a consumer holding it"
    );

    // One copy per publication and no more, however long the consumer keeps
    // reading -- the second edit lands in the copy the first one made, which
    // the consumer is not holding.
    emit.modify(|_| {});
    assert_eq!(clones.load(Ordering::SeqCst), 1, "and not once per edit");
    drop(snapshot);
    emit.modify(|_| {});
    assert_eq!(clones.load(Ordering::SeqCst), 1, "nor after they let go");

    // And one copy per publication *however many* consumers there are, which is
    // the half of the claim a single reader cannot show. Three snapshots of the
    // same value are three references, and the edit still copies once and
    // leaves all three reading what they were handed.
    let held = [watch.get(), watch.get(), watch.get()];
    emit.modify(|_| {});
    assert_eq!(clones.load(Ordering::SeqCst), 2, "with three holding it");
    drop(held);
}

/// How long the rigged `Clone` below takes, and the whole of what a `get`
/// issued against it waits for.
///
/// Long enough that the scheduling noise around it is a rounding error and
/// short enough that the suite does not notice.
const SLOW_CLONE: Duration = Duration::from_millis(300);

/// A value whose `Clone` is slow on purpose, and says when it started.
///
/// `modify` copies on write through `Arc::make_mut`, and it does that under the
/// lock every read also takes. Nothing in this crate can be timed reliably
/// against a real payload -- an audio-device list clones in microseconds and the
/// scheduler is noisier than that -- so the payload is rigged instead, and the
/// figure the test reports is the rig's own.
#[derive(Debug)]
struct SlowToClone {
    /// Set to `1` by `clone` before it starts, so a reader can issue its call
    /// at a known moment rather than after a sleep and a hope.
    cloning: Arc<AtomicUsize>,
}

impl Clone for SlowToClone {
    fn clone(&self) -> Self {
        self.cloning.store(1, Ordering::SeqCst);
        thread::sleep(SLOW_CLONE);
        Self {
            cloning: Arc::clone(&self.cloning),
        }
    }
}

/// `get` does not wait for a *publication*, and does wait for a `modify` that
/// is copying.
///
/// The README tabulates `get` and `changed_since` against `blocking_wait`, and
/// the honest column is qualified. A publication never holds a reader up:
/// `set` builds its value before the lock and drops the old one after, so no
/// line of a `T`'s own code runs inside. `modify` is the exception, and it is
/// the one the table has to say so about -- `Arc::make_mut` clones the whole `T`
/// under the lock when a consumer is holding the value being edited, and a
/// reader that arrives during that clone waits for all of it.
#[test]
fn a_get_waits_for_a_modify_that_is_copying_and_for_nothing_else() {
    let cloning = Arc::new(AtomicUsize::new(0));
    let (emit, watch) = channel(
        "rigged",
        SlowToClone {
            cloning: Arc::clone(&cloning),
        },
    );

    // With nobody holding the value, `make_mut` has the only reference and
    // edits in place: no clone, and a reader alongside it waits for nothing.
    let uncontended = Instant::now();
    emit.modify(|_| {});
    let uncontended = uncontended.elapsed();
    assert_eq!(
        cloning.load(Ordering::SeqCst),
        0,
        "it cloned with no reader"
    );
    let free_read = Instant::now();
    drop(watch.get());
    let free_read = free_read.elapsed();
    assert!(
        free_read < SLOW_CLONE / 10,
        "an uncontended `get` took {free_read:?}"
    );
    assert!(
        uncontended < SLOW_CLONE / 10,
        "an uncontended `modify` took {uncontended:?}"
    );

    // Now hold the value, so the next `modify` has to copy it first.
    let held = watch.get();
    let writer = thread::spawn(move || emit.modify(|_| {}));

    // Issued the moment the clone begins, which is the point of the flag: a
    // sleep would measure whatever was left of the clone rather than the clone.
    once("the copying `modify` starting its clone", || {
        cloning.load(Ordering::SeqCst) > 0
    });
    let contended = Instant::now();
    let read = within("a `get` issued during a copying `modify`", move || {
        watch.get()
    });
    let contended = contended.elapsed();
    drop(read);
    joined("the copying `modify`", writer);
    drop(held);

    // Most of the clone, allowing for however much of it had already elapsed
    // between the flag and the call. This is the claim: a read is not free
    // while a `modify` is copying.
    assert!(
        contended > SLOW_CLONE / 2,
        "a `get` issued during a {SLOW_CLONE:?} clone came back in {contended:?}"
    );
}
