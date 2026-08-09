//! What a publication costs, and what a consumer adds to it.
//!
//! ```sh
//! cargo bench -p corvid_signal
//! ```
//!
//! The number this exists to produce is the difference between publishing with
//! nobody watching and publishing while a consumer reads and copies every
//! value. That difference is what "a publication never waits for a consumer" is
//! worth, and it is the figure that moves if the value ever goes back to being
//! cloned under the lock -- which is what this crate did before the cell held an
//! `Arc`.
//!
//! The subject is a `Vec<String>`, which stands in for the audio-device list the
//! documentation reaches for. Building one is the publisher's own work rather
//! than the handoff's, so it happens in a setup closure that Criterion does not
//! time.

use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

use corvid_signal::{Watch, channel};

/// Entries in the list being published.
///
/// Large enough that copying it is a visible cost rather than noise, and small
/// enough that Criterion can build one per iteration without the run turning
/// into a coffee break.
const ENTRIES: usize = 50_000;

/// A list of the shape a device enumeration produces.
fn devices(tag: usize) -> Vec<String> {
    (0..ENTRIES).map(|i| format!("device {tag}:{i}")).collect()
}

/// A thread that reads a signal as fast as it can and takes its own copy of
/// everything it reads, until the flag is set.
///
/// The copy runs on this thread, outside every lock this crate has, which is
/// the property the contended row is measuring.
fn copying_consumer(watch: &Watch<Vec<String>>, stop: &Arc<AtomicUsize>) -> JoinHandle<()> {
    let watch = watch.clone();
    let stop = Arc::clone(stop);
    thread::spawn(move || {
        let mut seen = watch.seen_now();
        while stop.load(Ordering::SeqCst) == 0 {
            if let Some(value) = watch.changed_since(&mut seen) {
                black_box((*value).clone());
            }
        }
    })
}

/// Publishing, with nobody looking and with a consumer copying throughout.
fn publishing(c: &mut Criterion) {
    let mut group = c.benchmark_group("publish");

    let (emit, watch) = channel("audio devices", devices(0));
    group.bench_function("set, nobody watching", |b| {
        b.iter_batched(
            || devices(1),
            |value| emit.set(value),
            BatchSize::PerIteration,
        );
    });

    let stop = Arc::new(AtomicUsize::new(0));
    let reader = copying_consumer(&watch, &stop);
    group.bench_function("set, past a copying consumer", |b| {
        b.iter_batched(
            || devices(2),
            |value| emit.set(value),
            BatchSize::PerIteration,
        );
    });
    stop.store(1, Ordering::SeqCst);
    drop(reader.join());

    group.finish();
}

/// Observing, which is a reference-count bump, against the copy a consumer
/// takes afterwards if it needs to own the value.
fn observing(c: &mut Criterion) {
    let mut group = c.benchmark_group("observe");
    let (_emit, watch) = channel("audio devices", devices(0));

    group.bench_function("get", |b| {
        b.iter(|| drop(black_box(watch.get())));
    });
    group.bench_function("get then clone", |b| {
        b.iter(|| black_box((*watch.get()).clone()));
    });

    group.finish();
}

/// Editing in place against copying first, which is the one path where a `T`'s
/// own code runs inside the lock.
fn modifying(c: &mut Criterion) {
    let mut group = c.benchmark_group("modify");

    let (emit, _watch) = channel("audio devices", devices(0));
    group.bench_function("modify, sole reference", |b| {
        b.iter(|| emit.modify(|list| list.push(String::from("one more"))));
    });

    let (emit_cow, watch_cow) = channel("audio devices", devices(1));
    group.bench_function("modify, a reader holding the value", |b| {
        b.iter_batched(
            // The held handle is what makes `make_mut` see a second reference
            // and copy. Without it the edit lands in place and the row measures
            // the one above.
            || watch_cow.get(),
            |held| {
                emit_cow.modify(|list| list.push(String::from("one more")));
                drop(held);
            },
            BatchSize::PerIteration,
        );
    });

    group.finish();
}

criterion_group!(benches, publishing, observing, modifying);
criterion_main!(benches);
