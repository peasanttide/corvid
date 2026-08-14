//! How a test in this crate waits for another thread without being able to
//! wait forever.
//!
//! A run drives a loop on the thread it was called on and reports where it has
//! got to through a signal, so any test that watches a run in flight is a test
//! with a second thread in it. Every one of those has two ways to hang: the
//! condition it is polling for never arrives, and the thread it joins never
//! finishes. Both turn a failing assertion into a binary that never exits -- a
//! `headless` binary from an earlier run of this suite was found still wedged
//! six hours later, with nothing in any log to say which test it was.
//!
//! So nothing here waits on anything without one of these two. Each fails
//! inside [`PATIENCE`] with a sentence naming what was being waited for.
//!
//! [`drawing`] is the third, and it guards a different hang: several `wgpu`
//! devices built at once against a software rasteriser wedge against each
//! other, so a binary that builds one per test is a binary that intermittently
//! never finishes. It serialises them and puts the same deadline on each.
//!
//! These are twenty lines that `corvid_signal`'s own `tests/common` also has.
//! Sharing them would mean one of the two crates dev-depending on the other for
//! a sleep and a deadline, and a `tests/common` module is per-binary by
//! construction anyway.

#![allow(
    clippy::panic,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::print_stderr,
    reason = "the watchdog below prints as the process is aborting, where a tracing event needs a subscriber the harness has not installed and would not be flushed anyway"
)]

use std::{
    sync::{Mutex, OnceLock, PoisonError, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// How long something that is supposed to happen promptly is given before the
/// test calls it stuck.
///
/// Generous on purpose. Every caller is asserting the difference between
/// "happened" and "never going to", so the number only has to be longer than
/// the slowest scheduling hiccup a loaded CI box can produce and shorter than a
/// person's patience. The runs it guards take under a millisecond.
pub(crate) const PATIENCE: Duration = Duration::from_secs(10);

/// How often a wait looks again.
///
/// Long enough that ten seconds of waiting is ten thousand wakeups rather than
/// a core held flat, and short enough that a run of a few dozen ticks is not
/// measured through it.
const GLANCE: Duration = Duration::from_millis(1);

/// Waits until `ready` answers true, failing the test if it has not within
/// [`PATIENCE`].
///
/// `what` is what the failure says never arrived.
pub(crate) fn once(what: &str, ready: impl FnMut() -> bool) {
    let mut ready = ready;
    let started = Instant::now();
    while !ready() {
        assert!(
            started.elapsed() < PATIENCE,
            "{what} had not happened after {PATIENCE:?}, so it is not going to",
        );
        thread::sleep(GLANCE);
    }
}

/// Joins `handle` and hands back what it returned, failing the test if it had
/// not finished within [`PATIENCE`].
///
/// `what` is what the failure says the thread was still doing.
///
/// [`JoinHandle::join`] has no deadline, so a test written with it turns a run
/// that will never return into a binary that will never exit. This polls
/// [`JoinHandle::is_finished`] and leaves the thread where it is on a timeout,
/// because joining it is the hang being reported.
pub(crate) fn joined<T>(what: &str, handle: JoinHandle<T>) -> T {
    let started = Instant::now();
    while !handle.is_finished() {
        assert!(
            started.elapsed() < PATIENCE,
            "{what} had not finished after {PATIENCE:?}, so it is stuck rather than slow",
        );
        thread::sleep(GLANCE);
    }
    // Finished, so this returns without waiting. What it can still do is resume
    // a panic, which is the thread's own failure and carries its own message.
    match handle.join() {
        Ok(value) => value,
        Err(panicked) => std::panic::resume_unwind(panicked),
    }
}

/// How long after [`PATIENCE`] a binary that builds devices is given to die of
/// its own accord before [`impatience`] kills it.
///
/// Enough for a handful of threads to report a failed assertion and for the
/// harness to print them, and no more. Nothing is expected to be running by
/// then.
const GRACE: Duration = Duration::from_secs(30);

/// One graphics device at a time, for the whole test binary.
///
/// Every test that builds one passes on its own; what does not survive is
/// several of them at once. `tests/windowless.rs` builds three, and running that
/// binary forty times on a machine whose only adapter is Mesa's software
/// rasteriser wedged two of the forty -- the same failure `corvid_render`'s
/// `tests/offscreen.rs` guards against, in a smaller binary. Two hundred runs of
/// the same binary with this in place: none.
///
/// One at a time is also how a window uses a renderer, and it costs the binary
/// nothing measurable: the three runs it guards take under a second between
/// them.
static RENDERING: Mutex<()> = Mutex::new(());

/// Kills this process if it is still alive [`GRACE`] after the last device test
/// could possibly have finished.
///
/// Armed by the first [`drawing`] and no sooner, so a binary that never renders
/// never arms it.
///
/// `abort` and not `exit`, and the difference is the whole reason this exists.
/// A test that gives up on a wedged device leaves the thread where it is, and
/// that thread is inside a graphics driver -- so the orderly shutdown `exit`
/// performs runs the driver's own teardown, which waits for the thread that is
/// stuck. The result is a binary that prints its failures and then sits at a
/// hundred per cent of a core until somebody finds it. `abort` asks nothing of
/// anybody.
fn impatience() {
    static ARMED: OnceLock<()> = OnceLock::new();
    ARMED.get_or_init(|| {
        thread::spawn(|| {
            thread::sleep(PATIENCE + GRACE);
            eprintln!(
                "aborting: this binary was still alive {GRACE:?} after the last test's \
                 deadline, which means a thread abandoned inside the driver is holding the \
                 process open"
            );
            std::process::abort();
        });
    });
}

/// Runs one test's body on a thread of its own, one at a time across the
/// binary, failing the test if it had not finished within [`PATIENCE`].
///
/// `what` is what the failure says was still drawing.
///
/// The thread is abandoned rather than joined on a timeout, because joining it
/// is the hang being reported: the point is that this process reaches the end of
/// its run and exits with a failure somebody can read.
pub(crate) fn drawing(what: &str, work: impl FnOnce() + Send + 'static) {
    impatience();
    let (finished, done) = mpsc::channel();
    let started = Instant::now();
    thread::spawn(move || {
        // Taken inside the thread rather than outside it, so that waiting for a
        // test that has wedged holding it is under the same deadline as waiting
        // for the device: all of them start at once and all of them give up at
        // once, rather than each waiting its predecessor's timeout out.
        //
        // A poisoned lock is a test that already failed and said so. What is
        // guarded is a graphics device rather than a data structure, and it is
        // no more broken for the last holder having panicked.
        let _one_at_a_time = RENDERING.lock().unwrap_or_else(PoisonError::into_inner);
        work();
        // Fails only if this test has already given up and gone, which is the
        // timeout below and is already reported.
        let _ = finished.send(());
    });

    match done.recv_timeout(PATIENCE) {
        Ok(()) => {}
        Err(mpsc::RecvTimeoutError::Timeout) => panic!(
            "{what} had not finished after {PATIENCE:?} (waited {:?}), so the device it is \
             waiting on is not going to answer",
            started.elapsed(),
        ),
        // The sender went without sending, which is the body panicking; its own
        // message is already on the way out.
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("{what} panicked, and its own message is above")
        }
    }
}
