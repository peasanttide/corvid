//! What a publication costs, and what a consumer adds to it.
//!
//! ```sh
//! cargo run --release -p corvid_signal --example signal_bench
//! ```
//!
//! The `-p` is not decoration. This workspace has a package at its root, so
//! `--example` with no package named looks in that one alone and reports the
//! target as missing while telling you which crate it is in.
//!
//! The number this exists to produce is the last one: the difference between
//! publishing with nobody watching and publishing while one consumer reads and
//! copies every value. That difference is what "a publication never waits for a
//! consumer" is worth, and it is the figure that moves if the value ever goes
//! back to being cloned under the lock — which is what this crate did before the
//! cell held an `Arc`.
//!
//! The subject is a `Vec<String>` of 400 000 entries, which stands in for the
//! audio-device list the documentation reaches for. Nothing here is a benchmark
//! harness: it times loops with `Instant` and prints, because the comparison
//! between two rows is the point and it is not a close call. Building the list
//! is left outside every timed region — 400 000 `format!`s dwarf everything
//! else, and they are the publisher's own work rather than the handoff's.

#![allow(
    clippy::print_stdout,
    reason = "an example prints the numbers it measured, which is what makes it an example rather than a test"
)]

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use corvid_signal::{Emitter, Watch, channel};

/// Entries in the list being published. Large enough that copying it is
/// milliseconds rather than noise.
const ENTRIES: usize = 400_000;

/// How many publications each run makes.
const PUBLICATIONS: usize = 200;

/// How many times [`Watch::get`] is called to time one of them.
const READS: u32 = 1_000;

/// A list of the shape a device enumeration produces.
fn devices(tag: usize) -> Vec<String> {
    (0..ENTRIES).map(|i| format!("device {tag}:{i}")).collect()
}

/// Publishes [`PUBLICATIONS`] lists and reports how long the `set` calls took,
/// summed, and the slowest single one.
fn publish_all(emit: &Emitter<Vec<String>>) -> (Duration, Duration) {
    let mut total = Duration::ZERO;
    let mut worst = Duration::ZERO;
    for tag in 1..=PUBLICATIONS {
        let value = devices(tag);
        let started = Instant::now();
        emit.set(value);
        let took = started.elapsed();
        total += took;
        worst = worst.max(took);
    }
    (total, worst)
}

/// A thread that reads this signal as fast as it can and takes its own copy of
/// everything it reads, until the flag is set.
fn copying_consumer(watch: &Watch<Vec<String>>, stop: &Arc<AtomicUsize>) -> thread::JoinHandle<()> {
    let watch = watch.clone();
    let stop = Arc::clone(stop);
    thread::spawn(move || {
        let mut seen = watch.seen_now();
        while stop.load(Ordering::SeqCst) == 0 {
            if let Some(value) = watch.changed_since(&mut seen) {
                // The copy a consumer that needs to own the value takes. It
                // runs here, on this thread, outside every lock this crate has.
                let owned = (*value).clone();
                assert_eq!(owned.len(), ENTRIES);
            }
        }
    })
}

fn main() {
    let list = devices(0);

    // What one copy of the value costs, which is the unit the rest is read in.
    let started = Instant::now();
    let copy = list.clone();
    let one_copy = started.elapsed();
    println!("one copy of a {ENTRIES}-entry Vec<String>: {one_copy:?}");
    drop(copy);

    // What a consumer's read costs, now that it is a reference-count bump.
    let (emit, watch) = channel("audio devices", list);
    let started = Instant::now();
    for _ in 0..READS {
        drop(watch.get());
    }
    println!("one `Watch::get`: {:?}", started.elapsed() / READS);

    // One publication issued while a consumer is known to be mid-copy, which
    // is the case the whole design is about. The handshake is what makes it a
    // measurement rather than a coincidence: without it the publication lands
    // between two copies most of the time and reports whatever it likes.
    let copying = Arc::new(AtomicUsize::new(0));
    let copier = {
        let watch = watch.clone();
        let copying = Arc::clone(&copying);
        thread::spawn(move || {
            copying.store(1, Ordering::SeqCst);
            // How a consumer that needs to own the value writes it. The read is
            // this crate's and the copy is the consumer's.
            let owned = (*watch.get()).clone();
            copying.store(2, Ordering::SeqCst);
            assert_eq!(owned.len(), ENTRIES);
        })
    };
    while copying.load(Ordering::SeqCst) == 0 {
        thread::yield_now();
    }
    let started = Instant::now();
    emit.set(Vec::new());
    let mid_copy = started.elapsed();
    let _ = copier.join();
    println!("a publication issued while a consumer is copying: {mid_copy:?}");

    // The other side of the same coin, and the one the README's table has to
    // qualify: `modify` is copy-on-write, `Arc::make_mut` takes its copy under
    // the lock, and a reader that arrives while that copy is running waits for
    // it. This is the only path in the crate where a `T`'s own code runs inside
    // the lock, and the number below is one whole copy of the list.
    let (emit_cow, watch_cow) = channel("audio devices", devices(1));
    // Held so `make_mut` sees a second reference and has to copy. Without this
    // it edits in place and the measurement is of nothing.
    let held = watch_cow.get();
    let entering = Arc::new(AtomicUsize::new(0));
    let writer = {
        let entering = Arc::clone(&entering);
        thread::spawn(move || {
            entering.store(1, Ordering::SeqCst);
            emit_cow.modify(|list| list.push(String::from("one more")));
        })
    };
    while entering.load(Ordering::SeqCst) == 0 {
        thread::yield_now();
    }
    // A millisecond in, so the read lands inside the copy rather than racing
    // the thread that is about to start it. The copy itself is the row above.
    thread::sleep(Duration::from_millis(1));
    let started = Instant::now();
    drop(watch_cow.get());
    let during_modify = started.elapsed();
    let _ = writer.join();
    drop(held);
    println!("a `Watch::get` issued while a `modify` is copying: {during_modify:?}");

    // The baseline: publishing with nobody looking. Most of what this measures
    // is freeing the list each publication replaced, which is 400 000
    // deallocations and is the publisher's own work either way.
    let (alone, alone_worst) = publish_all(&emit);
    println!(
        "{PUBLICATIONS} publications with nobody watching: {alone:?} total, {:?} each, \
         {alone_worst:?} worst",
        alone / u32::try_from(PUBLICATIONS).unwrap_or(1),
    );

    // And the same run with a consumer reading and copying throughout.
    let stop = Arc::new(AtomicUsize::new(0));
    let reader = copying_consumer(&watch, &stop);
    thread::sleep(Duration::from_millis(50));
    let (contended, contended_worst) = publish_all(&emit);
    stop.store(1, Ordering::SeqCst);
    let _ = reader.join();

    println!(
        "{PUBLICATIONS} publications past a copying consumer: {contended:?} total, {:?} each, \
         {contended_worst:?} worst",
        contended / u32::try_from(PUBLICATIONS).unwrap_or(1),
    );
    println!(
        "what the consumer cost the publisher: {:?} over {PUBLICATIONS} publications",
        contended.saturating_sub(alone),
    );
}
