//! The watchdog every device test runs under.
//!
//! The seam against `mod.rs` is the device: nothing here opens one. What it
//! does is put a deadline on a thread that has, because a wedged driver is the
//! one failure the rest of this harness cannot report.

use super::{GRACE, PATIENCE, RENDERING};
use std::{
    sync::{OnceLock, PoisonError, mpsc},
    thread,
    time::Instant,
};

/// Kills this process if it is still alive [`GRACE`] after the last test could
/// possibly have finished.
///
/// Armed by the first [`drawing`] and no sooner, so a run that never renders
/// never arms it.
///
/// `abort` and not `exit`, and the difference is the whole reason this exists. A
/// test that gives up on a wedged device leaves the thread where it is, and that
/// thread is inside a graphics driver -- so the orderly shutdown `exit` performs
/// runs the driver's own teardown, which waits for the thread that is stuck.
/// The observed result was a binary that printed its failures and then sat at a
/// hundred per cent of a core until somebody found it. `abort` asks nothing of
/// anybody.
pub(crate) fn impatience() {
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

/// Runs one test's body on a thread of its own, failing the test if it had not
/// finished within [`PATIENCE`].
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
        // for the device.
        //
        // A poisoned lock is a test that already failed and said so. What is
        // guarded is a graphics device rather than a data structure, and it is
        // no more broken for the last holder having panicked, so the rest of
        // the file still runs.
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
