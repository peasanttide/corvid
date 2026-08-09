//! That nobody waits for anybody, and that everybody parked is woken.
//!
//! A latest-value cell's whole claim is that publishing never waits for a
//! consumer and that a consumer that never polls costs a publisher nothing. A
//! test about not waiting fails by hanging unless something stops it, so
//! nothing here waits on anything without one of `common`'s three backstops:
//! [`common::within`] around a call that blocks, [`common::joined`] around a
//! thread that may never finish, and [`common::once`] around a flag another
//! thread has to set.

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
    time::Duration,
};

use common::{joined, once, within};
use corvid_signal::{Emitter, channel};

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
    // them, so something has to keep publishing until they are all out -- which
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
    // reaching it would park the test thread for good -- which is exactly the
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
    // the parking itself, which no thread can announce from the inside -- a
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
