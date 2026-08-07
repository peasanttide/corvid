//! What a trace of this crate actually contains.
//!
//! The README claims a publication opens a span that names the signal and that
//! an observation leaves an event beside it, and a claim about a trace is only
//! checkable by reading one. So these tests install a subscriber that records
//! every span and event, and assert on what it collected.
//!
//! Every test in this file installs one before it touches a signal, on purpose:
//! `tracing` caches a callsite's interest the first time it is reached, so a
//! test in this binary that published with no subscriber in place could leave
//! the callsite cached as uninteresting for the tests that run after it.

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]

use std::{
    fmt,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use corvid_signal::{Seen, channel};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

/// One span or one event, as a subscriber saw it.
#[derive(Clone, Debug, Default)]
struct Recorded {
    /// The callsite's name — `corvid_signal.set` and its neighbours.
    name: String,
    /// The callsite's level, as `tracing` prints it: `DEBUG` on a publication's
    /// span and `TRACE` on an observation's event.
    ///
    /// Recorded because the README tabulates it, and a column in a table is a
    /// claim like any other. It is also the one part of a callsite that is
    /// cheap to get wrong and impossible to notice: a `DEBUG` event where the
    /// table says `TRACE` costs nothing until a subscriber filtered at `DEBUG`
    /// starts printing one line per poll.
    level: String,
    /// Every field, in the order it arrived, including the ones filled in after
    /// the span was opened.
    fields: Vec<(String, String)>,
}

impl Recorded {
    /// What one field was recorded as, and [`None`] if it was never recorded.
    fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(recorded, _)| recorded == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Everything a subscriber collected, in the order it arrived.
#[derive(Default)]
struct Log {
    /// Spans, indexed by the identifier this subscriber handed out minus one,
    /// so a field recorded after the span opened lands on the right one.
    spans: Mutex<Vec<Recorded>>,
    /// Events, which carry all their fields at once and need no identifier.
    events: Mutex<Vec<Recorded>>,
    /// The last span identifier handed out.
    handed_out: AtomicU64,
}

impl Log {
    fn spans(&self) -> Vec<Recorded> {
        lock(&self.spans).clone()
    }

    fn events(&self) -> Vec<Recorded> {
        lock(&self.events).clone()
    }
}

/// The lock, with poisoning ignored: a panic in one test should report as that
/// test failing rather than as every later one failing to read the log.
fn lock<T>(what: &Mutex<T>) -> MutexGuard<'_, T> {
    what.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A subscriber that records rather than prints.
struct Recorder(Arc<Log>);

impl Subscriber for Recorder {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let mut fields = Fields::default();
        span.record(&mut fields);
        lock(&self.0.spans).push(Recorded {
            name: span.metadata().name().to_owned(),
            level: span.metadata().level().to_string(),
            fields: fields.0,
        });
        Id::from_u64(self.0.handed_out.fetch_add(1, Ordering::SeqCst) + 1)
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        // Identifiers are handed out from one upwards, so the span this is
        // about is at one less than its identifier.
        if let Ok(index) = usize::try_from(span.into_u64().saturating_sub(1))
            && let Some(recorded) = lock(&self.0.spans).get_mut(index)
        {
            recorded.fields.extend(fields.0);
        }
    }

    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        lock(&self.0.events).push(Recorded {
            name: event.metadata().name().to_owned(),
            level: event.metadata().level().to_string(),
            fields: fields.0,
        });
    }

    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

/// Collects a span's or an event's fields as text.
#[derive(Default)]
struct Fields(Vec<(String, String)>);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }

    /// Strings without the quotes a `Debug` would add, so an assertion below
    /// reads `Some("surface")` rather than `Some("\"surface\"")`.
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }
}

/// Runs `body` with a recording subscriber installed on this thread, and hands
/// back what it collected.
fn traced(body: impl FnOnce()) -> Arc<Log> {
    let log = Arc::new(Log::default());
    tracing::subscriber::with_default(Recorder(Arc::clone(&log)), body);
    log
}

#[test]
fn every_publication_opens_a_span_that_names_the_signal_and_the_sequence() {
    let log = traced(|| {
        let (emit, watch) = channel("surface", 0_u32);
        emit.set(7);
        emit.modify(|value| *value += 1);
        let mut seen = Seen::default();
        assert_eq!(watch.changed_since(&mut seen).as_deref(), Some(&8));
    });

    let spans = log.spans();
    assert_eq!(spans.len(), 2, "{spans:?}");

    // The initial value is sequence one, so the first publication is two.
    assert_eq!(spans[0].name, "corvid_signal.set");
    assert_eq!(spans[0].level, "DEBUG");
    assert_eq!(spans[0].field("signal"), Some("surface"));
    assert_eq!(spans[0].field("sequence"), Some("2"));

    assert_eq!(spans[1].name, "corvid_signal.modify");
    assert_eq!(spans[1].level, "DEBUG");
    assert_eq!(spans[1].field("signal"), Some("surface"));
    assert_eq!(spans[1].field("sequence"), Some("3"));
}

#[test]
fn an_observation_leaves_the_far_end_of_the_handoff_in_the_trace() {
    let log = traced(|| {
        let (emit, watch) = channel("surface", 0_u32);
        let mut seen = watch.seen_now();
        emit.set(7);
        assert_eq!(watch.changed_since(&mut seen).as_deref(), Some(&7));
    });

    let events = log.events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].name, "corvid_signal.observed");
    // A level below the publication's, and deliberately: a poll that saw
    // something is the common case and a publication is the rarer one.
    assert_eq!(events[0].level, "TRACE");
    assert_eq!(events[0].field("signal"), Some("surface"));
    // The same sequence number the publication's span carried, which is what
    // makes the two ends of one handoff joinable in a trace.
    assert_eq!(events[0].field("sequence"), Some("2"));
}

#[test]
fn a_poll_that_saw_nothing_leaves_nothing() {
    let log = traced(|| {
        let (_emit, watch) = channel("surface", 0_u32);
        let mut seen = watch.seen_now();
        for _ in 0..64 {
            assert_eq!(watch.changed_since(&mut seen).as_deref(), None);
        }
    });

    // A consumer polling once a frame is the ordinary case, and an event per
    // poll would bury the ones that mean something.
    assert!(log.events().is_empty(), "{:?}", log.events());
    assert!(log.spans().is_empty(), "{:?}", log.spans());
}

#[test]
fn two_signals_are_told_apart_by_their_label() {
    let log = traced(|| {
        let (surface, _) = channel("surface", 0_u32);
        let (peers, _) = channel("peers", 0_u32);
        surface.set(1);
        peers.set(2);
    });

    let spans = log.spans();
    assert_eq!(spans.len(), 2, "{spans:?}");
    assert_eq!(spans[0].field("signal"), Some("surface"));
    assert_eq!(spans[1].field("signal"), Some("peers"));
}
