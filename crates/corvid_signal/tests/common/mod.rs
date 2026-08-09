//! What more than one of these test binaries needs.
//!
//! Three backstops, and nothing in these tests may wait on anything without one
//! of them. Every property in this crate that is worth checking is a property
//! about a thread not waiting, and the failure mode of all of them is a test
//! binary that never returns -- a red build with no message, or a CI job killed
//! by a timeout an hour later. That is not hypothetical: a `threads` binary from
//! an earlier run of this suite was found still wedged nine and a half hours
//! later, and the agent that started it had reported the run clean.
//!
//! So there is one of these around every wait. [`within`] is for a call that
//! blocks, [`joined`] for a thread that may never finish, and [`once`] for a
//! flag another thread has to set. All three fail inside [`PATIENCE`] with a
//! sentence naming what was being waited for, which is the difference between a
//! test that reports and a test that has to be found with `ps`.

#![allow(
    clippy::panic,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]
#![allow(
    dead_code,
    reason = "each integration test binary compiles this module separately, so anything only one of them uses is dead in the others"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "this module is private to each test binary, so pub(crate) and pub are equivalent -- pub(crate) is the one rustc's unreachable_pub asks for, and the two lints cannot both be satisfied"
)]

use std::{
    sync::mpsc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// How long a call that is supposed to return promptly is given before the test
/// calls it blocked.
///
/// Generous on purpose. Every test that uses it is asserting the difference
/// between "returned" and "parked forever", so the number only has to be longer
/// than the slowest scheduling hiccup a loaded CI box can produce and shorter
/// than a person's patience. It is not a performance bound and nothing here
/// asserts anything took less than a millisecond.
pub(crate) const PATIENCE: Duration = Duration::from_secs(10);

/// The deadline for a guard wrapped around a whole session rather than around
/// one call, which is [`within_for`]'s only caller.
///
/// Longer than [`PATIENCE`] so that the named guard inside such a session is the
/// one that fires. See [`within_for`].
pub(crate) const SIEGE: Duration = Duration::from_secs(30);

/// Runs `call` on a thread of its own and hands back what it returned, failing
/// the test if it had not returned within [`PATIENCE`].
///
/// `what` is what the failure says was blocked.
///
/// The thread is abandoned rather than joined when it times out, because
/// joining it is exactly the hang this exists to avoid -- the test process
/// carries a parked thread to the end of the run and the assertion that already
/// failed is what gets reported.
pub(crate) fn within<T: Send + 'static>(
    what: &str,
    call: impl FnOnce() -> T + Send + 'static,
) -> T {
    within_for(PATIENCE, what, call)
}

/// [`within`] with the deadline spelled out.
///
/// The one thing this is for is a guard around a whole session that has named
/// guards inside it. Both would otherwise be counting the same ten seconds, and
/// the outer one -- which started first and knows least -- would be the one to
/// report. Giving the outer guard more rope is what makes the message name the
/// thread that is stuck rather than the session it is stuck in.
pub(crate) fn within_for<T: Send + 'static>(
    patience: Duration,
    what: &str,
    call: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (finished, done) = mpsc::channel();
    let started = Instant::now();
    thread::spawn(move || {
        // The send fails only if the test thread has already given up and gone,
        // which is the timeout case and is already reported.
        let _ = finished.send(call());
    });

    match done.recv_timeout(patience) {
        Ok(value) => value,
        Err(mpsc::RecvTimeoutError::Timeout) => panic!(
            "{what} had not returned after {patience:?} (waited {:?}), so it is blocked rather than slow",
            started.elapsed(),
        ),
        // The sender went without sending, which means the call panicked and
        // the default hook has already printed why. Saying so is what keeps a
        // failed assertion inside the call from being reported as a hang.
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("{what} panicked, and its own message is above")
        }
    }
}

/// How often a backstop that has to poll looks again.
///
/// Short enough that nothing here measures the sleep instead of the thing it is
/// waiting for -- the shortest interval any of these tests cares about is a
/// fifty-millisecond publication delay -- and long enough that a wait of ten
/// seconds is ten thousand wakeups rather than a core held flat.
const GLANCE: Duration = Duration::from_millis(1);

/// Joins `handle` and hands back what it returned, failing the test if it had
/// not finished within [`PATIENCE`].
///
/// `what` is what the failure says the thread was still doing.
///
/// [`JoinHandle::join`] has no deadline, so a teardown written with it turns a
/// thread that will never exit into a binary that will never exit. This polls
/// [`JoinHandle::is_finished`] instead and leaves the thread where it is on a
/// timeout, for [`within`]'s reason: joining it is the hang being reported.
pub(crate) fn joined<T>(what: &str, handle: JoinHandle<T>) -> T {
    let started = Instant::now();
    while !handle.is_finished() {
        assert!(
            started.elapsed() < PATIENCE,
            "{what} had not finished after {PATIENCE:?}, so it is stuck rather than slow",
        );
        thread::sleep(GLANCE);
    }
    // The thread is finished, so this join returns without waiting. What it can
    // still do is resume a panic, which is the thread's own failure and carries
    // the thread's own message.
    match handle.join() {
        Ok(value) => value,
        Err(panicked) => std::panic::resume_unwind(panicked),
    }
}

/// Waits until `ready` answers true, failing the test if it has not within
/// [`PATIENCE`].
///
/// `what` is what the failure says never became true.
///
/// This is the shape of every "wait until the other thread has got there" in
/// these tests. Written as a bare `while !ready() {}` it is the quietest hang of
/// the three: nothing is blocked, nothing is parked, and a core spins until
/// somebody notices the job has been running for an hour.
pub(crate) fn once(what: &str, ready: impl Fn() -> bool) {
    let started = Instant::now();
    while !ready() {
        assert!(
            started.elapsed() < PATIENCE,
            "{what} had not happened after {PATIENCE:?}, so it is not going to",
        );
        thread::sleep(GLANCE);
    }
}
