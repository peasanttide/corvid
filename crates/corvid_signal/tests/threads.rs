//! Eight publishers and eight consumers on one signal at once.
//!
//! The value is a struct whose three fields have to agree with each other,
//! because a single integer cannot tear and a test built on one would pass
//! against a cell that assembled its answer out of two publications. Each
//! writer seals its author and its ticket together; a reader that saw the
//! author from one publication and the ticket from another breaks the seal, and
//! `the_seal_tells_a_mixed_reading_from_a_whole_one` is the check that the seal
//! really does break.
//!
//! Be exact about what a pass buys. The cell is a `Mutex`, so tearing is not
//! something this implementation can do -- the assertion is a guard on whatever
//! replaces it, and on the day somebody reaches for two atomics because the
//! lock showed up in a profile. What it does test *today* is the rest of the
//! list: that a consumer never sees a value nobody published, never sees one
//! publication twice, and never sees a signal go backwards.

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use common::{SIEGE, joined, within_for};
use corvid_signal::{Emitter, Seen, Watch, channel};

/// How many threads publish.
const EMITTERS: usize = 8;
/// How many threads observe.
const WATCHERS: usize = 8;
/// How many publications each emitter makes.
const WRITES: u64 = 2_000;
/// The author of the publications that wake the watchers at shutdown. Outside
/// the range of real authors, so a reading can say which phase it came from.
const CLOSER: u64 = EMITTERS as u64;

/// One publication: who wrote it, which of their writes it is, and a seal over
/// both.
///
/// The seal is what makes a torn read visible. Every field is written by one
/// thread in one go, so any reading whose seal does not match the author and
/// ticket beside it was assembled out of more than one publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Reading {
    author: u64,
    ticket: u64,
    seal: u64,
}

impl Reading {
    const fn new(author: u64, ticket: u64) -> Self {
        Self {
            author,
            ticket,
            seal: seal_of(author, ticket),
        }
    }

    /// Whether the three fields came from one publication.
    const fn is_whole(self) -> bool {
        self.seal == seal_of(self.author, self.ticket)
    }
}

/// A bijection in each argument separately, so changing either one alone
/// changes the seal.
const fn seal_of(author: u64, ticket: u64) -> u64 {
    author.rotate_left(29).wrapping_mul(0xbea2_25f9_eb34_556d)
        ^ ticket.wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

#[test]
fn the_seal_tells_a_mixed_reading_from_a_whole_one() {
    let first = Reading::new(3, 11);
    let second = Reading::new(5, 12);
    assert!(first.is_whole() && second.is_whole());

    // Every way of taking some fields from one publication and the rest from
    // another. If any of these read as whole, the stress test below is
    // asserting nothing.
    let mixed = [
        Reading {
            author: second.author,
            ..first
        },
        Reading {
            ticket: second.ticket,
            ..first
        },
        Reading {
            seal: second.seal,
            ..first
        },
        Reading {
            seal: first.seal,
            ..second
        },
        Reading {
            ticket: first.ticket,
            ..second
        },
        Reading {
            author: first.author,
            ..second
        },
    ];

    for reading in mixed {
        assert!(!reading.is_whole(), "{reading:?} passed as whole");
    }
}

/// What one consumer checks about the sequence of readings it observed.
///
/// Kept per consumer rather than shared, because these are all statements about
/// what *one* watcher saw in the order it saw it.
#[derive(Default)]
struct Ledger {
    /// The last ticket seen from each author, indexed by author.
    latest: [Option<u64>; EMITTERS + 1],
    /// How many readings were observed at all.
    observed: usize,
    /// How many of them came from a real emitter rather than from the shutdown
    /// publisher.
    real: usize,
}

impl Ledger {
    /// Folds one observation in, or says what was wrong with it.
    fn saw(&mut self, reading: Reading) -> Result<(), String> {
        self.observed += 1;

        if !reading.is_whole() {
            return Err(format!("{reading:?} was assembled out of two publications"));
        }
        if reading.author > CLOSER {
            return Err(format!("{reading:?} is from an author nobody ran"));
        }
        if reading.author != CLOSER {
            if reading.ticket > WRITES {
                return Err(format!("{reading:?} is a ticket nobody wrote"));
            }
            self.real += 1;
        }

        let author = usize::try_from(reading.author).unwrap();
        if let Some(before) = self.latest[author] {
            // Every author's tickets increase, the shutdown publisher's
            // included -- which is what makes this strict. A consumer told about
            // one publication twice sees the same ticket twice, and that is the
            // whole of what a `Seen` that failed to advance looks like from out
            // here.
            if reading.ticket <= before {
                return Err(format!(
                    "{reading:?} came after author {author} was already at ticket {before}",
                ));
            }
        }
        self.latest[author] = Some(reading.ticket);
        Ok(())
    }
}

/// Publishes `WRITES` readings, alternating the two ways of publishing so both
/// are under contention.
fn publish(emit: &Emitter<Reading>, author: u64) {
    for ticket in 1..=WRITES {
        if ticket % 2 == 0 {
            emit.set(Reading::new(author, ticket));
        } else {
            emit.modify(|reading| *reading = Reading::new(author, ticket));
        }
    }
}

/// Observes until told to stop, checking every reading against the ledger.
///
/// `parks` chooses which of the two ways of consuming this watcher uses, so
/// half of them are asleep on the condition variable and half are polling.
fn observe(watch: &Watch<Reading>, stop: &AtomicBool, parks: bool) -> Result<Ledger, String> {
    let mut ledger = Ledger::default();
    let mut seen = Seen::default();

    while !stop.load(Ordering::SeqCst) {
        let reading = if parks {
            watch.blocking_wait(&mut seen)
        } else {
            let Some(reading) = watch.changed_since(&mut seen) else {
                thread::yield_now();
                continue;
            };
            reading
        };
        ledger.saw(*reading)?;
    }

    Ok(ledger)
}

#[test]
fn eight_emitters_and_eight_watchers_agree_about_every_value() {
    // The whole session runs under a watchdog rather than only the join at the
    // end of it. Nearly every line below is a publication, an observation or a
    // join, and any of the three that stops returning turns this test into a
    // binary that never exits -- which was not a hypothetical: an early draft
    // guarded only the last join, and a `blocking_wait` that held the lock
    // while it waited hung the whole run rather than failing it.
    //
    // Every join inside is guarded by name as well, and this outer one is the
    // catch-all for the waits that are not joins -- `get` takes the same lock a
    // publication does. It is given [`SIEGE`] rather than the usual patience so
    // that the guard which can say *which* thread is stuck reports first.
    within_for(
        SIEGE,
        "eight emitters and eight watchers on one signal",
        contended,
    );
}

/// The session itself, so that the test above is one call under the watchdog.
#[allow(
    clippy::needless_collect,
    reason = "the two collects are what make this a stress test: each spawns all eight threads before any of them is joined, and the lazy iterator clippy suggests would spawn one, join it, and spawn the next -- one thread at a time and no contention at all"
)]
fn contended() {
    let (emit, watch) = channel("readings", Reading::new(CLOSER, 0));
    let stop = Arc::new(AtomicBool::new(false));

    let watchers: Vec<_> = (0..WATCHERS)
        .map(|which| {
            let watch = watch.clone();
            let stop = Arc::clone(&stop);
            thread::spawn(move || observe(&watch, &stop, which % 2 == 0))
        })
        .collect();

    let emitters: Vec<_> = (0..EMITTERS)
        .map(|author| {
            let emit = emit.clone();
            thread::spawn(move || publish(&emit, author as u64))
        })
        .collect();

    for (author, emitter) in emitters.into_iter().enumerate() {
        joined(
            &format!("emitter {author} publishing its {WRITES} readings"),
            emitter,
        );
    }

    // Every emitter's last publication is its ticket `WRITES`, and the last one
    // to reach the cell is the one still in it. A cell that had kept anything
    // other than the latest lands somewhere else.
    let settled = watch.get();
    assert!(settled.is_whole(), "{settled:?} was left half-written");
    assert_eq!(settled.ticket, WRITES, "{settled:?} is not a final write");
    assert!(
        settled.author < CLOSER,
        "{settled:?} is not from an emitter"
    );

    // A quiet stretch with the emitters gone and nothing publishing, which is
    // the one window in this test where a polling watcher is certainly polling
    // faster than anything is being published. A consumer that failed to
    // advance its own `Seen` reports the settled reading over and over here,
    // and the ledger holds every author to a ticket that climbs.
    thread::sleep(Duration::from_millis(20));

    // The watchers exit on the flag and the parked half only notices when
    // something wakes them, so something has to keep publishing until they are
    // all out.
    let all_out = Arc::new(AtomicUsize::new(0));
    let closer = {
        let all_out = Arc::clone(&all_out);
        thread::spawn(move || {
            // Its ticket climbs like everybody else's, so the polling watchers
            // are still held to seeing each publication once while this is the
            // only thing publishing -- which is the phase in which they poll far
            // more often than anything is published.
            let mut ticket = 0;
            while all_out.load(Ordering::SeqCst) == 0 {
                ticket += 1;
                emit.set(Reading::new(CLOSER, ticket));
                thread::sleep(Duration::from_millis(1));
            }
        })
    };
    stop.store(true, Ordering::SeqCst);

    let ledgers: Vec<_> = watchers
        .into_iter()
        .enumerate()
        .map(|(which, watcher)| joined(&format!("watcher {which} noticing the stop flag"), watcher))
        .collect();
    all_out.store(1, Ordering::SeqCst);
    joined("the thread waking the watchers at shutdown", closer);

    let mut real = 0;
    for (which, ledger) in ledgers.into_iter().enumerate() {
        let ledger = ledger.unwrap_or_else(|complaint| panic!("watcher {which}: {complaint}"));
        assert!(ledger.observed > 0, "watcher {which} observed nothing");
        real += ledger.real;
    }

    // The one assertion here about scheduling rather than about the type: with
    // sixteen thousand publications and eight polling threads, a run in which
    // no consumer ever saw one is a run that tested nothing, and should say so
    // rather than pass.
    assert!(real > 0, "no watcher observed a single real publication");
}
