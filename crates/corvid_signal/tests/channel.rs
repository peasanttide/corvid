//! What a latest-value cell promises, one property per test.
//!
//! Everything here runs on one or two threads and is about semantics. The
//! contention is in `tests/threads.rs` and the spans are in `tests/tracing.rs`.
//!
//! Several of these are about a thread *not* waiting, and a test about not
//! waiting fails by hanging unless something stops it. `common`'s three
//! backstops are that something, and nothing here waits on anything without
//! one: [`common::within`] around a call that blocks, [`common::joined`] around
//! a thread that may never finish, and [`common::once`] around a flag another
//! thread has to set.

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use std::{
    panic::{self, AssertUnwindSafe},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use common::{joined, once, within};
use corvid_signal::{Emitter, Seen, channel};

/// How many publications the tests that are about volume make.
const MANY: u64 = 10_000;

/// A value that says how many of itself are alive.
///
/// This is what turns "never grows" and "drops intermediate values" from
/// descriptions into assertions. A cell that queued its publications, or kept
/// the previous one to diff against, or handed the old value to the wakeup path
/// and forgot it there, all read as a live count that goes up with the number
/// of publications.
#[derive(Debug)]
struct Counted {
    /// Shared with the test, incremented on construction and on clone,
    /// decremented on drop.
    live: Arc<AtomicUsize>,
    /// Which publication this is, so a test can tell the values apart.
    tag: u64,
}

impl Counted {
    fn new(live: &Arc<AtomicUsize>, tag: u64) -> Self {
        live.fetch_add(1, Ordering::SeqCst);
        Self {
            live: Arc::clone(live),
            tag,
        }
    }
}

impl Clone for Counted {
    fn clone(&self) -> Self {
        Self::new(&self.live, self.tag)
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::SeqCst);
    }
}

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

#[test]
fn a_watcher_that_never_polls_does_not_hold_up_an_emitter() {
    let live = Arc::new(AtomicUsize::new(0));
    let (emit, watch) = channel("readings", Counted::new(&live, 0));

    let counter = Arc::clone(&live);
    within("ten thousand publications with nobody polling", move || {
        for tag in 1..=MANY {
            emit.set(Counted::new(&counter, tag));
        }
    });

    // The cell holds one value. Not `MANY`, which is what a queue holds, and
    // not two, which is what a cell keeping the previous value holds.
    assert_eq!(
        live.load(Ordering::SeqCst),
        1,
        "values alive after {MANY} sets"
    );
    assert_eq!(watch.get().tag, MANY);
    // And `get` handed out a handle on that one value rather than a copy of
    // it, so the count did not move.
    assert_eq!(live.load(Ordering::SeqCst), 1);
}

#[test]
fn a_watcher_parked_in_blocking_wait_does_not_hold_up_an_emitter() {
    let (emit, watch) = channel("readings", 0_u64);
    let stop = Arc::new(AtomicUsize::new(0));

    // Eight threads asleep on this signal, which is the shape the crate is for:
    // a thread with nothing to do until something is published.
    let parked: Vec<_> = (0..8)
        .map(|_| {
            let watch = watch.clone();
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                let mut seen = watch.seen_now();
                while stop.load(Ordering::SeqCst) == 0 {
                    let _ = watch.blocking_wait(&mut seen);
                }
            })
        })
        .collect();

    let publisher = emit.clone();
    within(
        "ten thousand publications past eight parked watchers",
        move || {
            for value in 1..=MANY {
                publisher.set(value);
            }
        },
    );

    // Everything below is teardown; the assertion above has been made. The
    // parked threads exit on the flag, and only notice it when something wakes
    // them, so something has to keep publishing until they are all out — which
    // is the shutdown the documentation says a thread parked here needs.
    stop.store(1, Ordering::SeqCst);
    let all_out = Arc::new(AtomicUsize::new(0));
    let waker = {
        let all_out = Arc::clone(&all_out);
        thread::spawn(move || {
            while all_out.load(Ordering::SeqCst) == 0 {
                emit.set(0);
                thread::sleep(Duration::from_millis(1));
            }
        })
    };

    within("joining eight parked watchers", move || {
        for handle in parked {
            handle.join().unwrap();
        }
    });
    all_out.store(1, Ordering::SeqCst);
    joined("the thread waking the parked watchers", waker);
}

#[test]
fn blocking_wait_wakes_when_something_is_published() {
    let (emit, watch) = channel("late", 0_u32);
    let seen = watch.seen_now();

    let publisher = thread::spawn(move || {
        // Long enough that the waiter is parked rather than merely behind, so
        // this is a test about the wakeup and not about the sequence check the
        // next test covers.
        thread::sleep(Duration::from_millis(50));
        emit.set(7);
    });

    let woken = within("blocking_wait across a publication", move || {
        let mut seen = seen;
        watch.blocking_wait(&mut seen)
    });

    assert_eq!(*woken, 7);
    joined("the thread publishing to the woken waiter", publisher);
}

#[test]
fn blocking_wait_does_not_park_when_it_is_already_behind() {
    let (emit, watch) = channel("early", 0_u32);
    let seen = watch.seen_now();

    emit.set(7);
    // Nobody is left who could ever wake anybody, so a `blocking_wait` that
    // parked before checking would park for good.
    drop(emit);

    let value = within("blocking_wait after a publication it missed", move || {
        let mut seen = seen;
        watch.blocking_wait(&mut seen)
    });

    assert_eq!(*value, 7);
}

/// A value whose `Drop` publishes to the signal it was published on.
///
/// The shape this stands in for is a resource handle that reports its own
/// retirement — the thing being dropped is what knows it is gone. It is the one
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
    // reading — the second edit lands in the copy the first one made, which
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

/// Both handles hand back the label, and print it without reading the value.
///
/// The `Debug` half is not decoration. Both implementations name the signal and
/// deliberately stop there, because reading the value means taking the lock and
/// the likeliest place a handle gets formatted is a `modify` closure that is
/// already holding it — so a `Debug` that reached for the value would turn a
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

#[test]
fn one_set_wakes_every_parked_consumer_and_not_one_of_them() {
    one_publication_wakes_everybody(|emit| emit.set(7));
}

/// The same for the other publishing path, because it is a separate call to
/// the condition variable and a separate chance to wake one of eight.
#[test]
fn one_modify_wakes_every_parked_consumer_and_not_one_of_them() {
    one_publication_wakes_everybody(|emit| emit.modify(|value| *value = 7));
}

/// Parks eight threads on one signal, publishes **once** through `publish`, and
/// fails unless all eight came back with what was published.
///
/// None of the eight loops, so a wakeup that reached one of them leaves seven
/// parked for good and the join is what reports it. That is the whole point:
/// `notify_one` in place of `notify_all` is invisible to every other test in
/// this crate, because every one of them publishes more times than there are
/// consumers.
fn one_publication_wakes_everybody(publish: impl FnOnce(&Emitter<u32>)) {
    /// How many threads park on the one publication below.
    const PARKED: usize = 8;

    let (emit, watch) = channel("late", 0_u32);
    let about_to_park = Arc::new(AtomicUsize::new(0));

    // Each of these parks once and exits. None of them loops, so a wakeup that
    // reached one thread and not the other seven leaves seven threads parked
    // for good and the join below is what reports it.
    //
    // The counter and not a `Barrier`: a barrier is the one wait in this file
    // that cannot be given a deadline, so a thread that panicked before
    // reaching it would park the test thread for good — which is exactly the
    // failure this file exists to stop shipping. Incrementing after the `Seen`
    // is taken carries the same ordering the barrier did, because nothing here
    // proceeds until the count reaches `PARKED`.
    let parked: Vec<_> = (0..PARKED)
        .map(|_| {
            let watch = watch.clone();
            let about_to_park = Arc::clone(&about_to_park);
            thread::spawn(move || {
                let mut seen = watch.seen_now();
                about_to_park.fetch_add(1, Ordering::SeqCst);
                *watch.blocking_wait(&mut seen)
            })
        })
        .collect();

    // Every thread has taken its `Seen` and reached the call. The wait is for
    // the parking itself, which no thread can announce from the inside — a
    // thread that had not parked yet would return on the sequence number rather
    // than on the wakeup, and would pass this test for the wrong reason.
    once("all eight threads reaching the call", || {
        about_to_park.load(Ordering::SeqCst) >= PARKED
    });
    thread::sleep(Duration::from_millis(200));

    // One publication. Not one per parked thread, which is what would let a
    // `notify_one` pass.
    publish(&emit);
    drop(emit);

    let woken = within("eight threads woken by one publication", move || {
        parked
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(woken, vec![7; PARKED]);
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
/// against a real payload — an audio-device list clones in microseconds and the
/// scheduler is noisier than that — so the payload is rigged instead, and the
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
/// the one the table has to say so about — `Arc::make_mut` clones the whole `T`
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
