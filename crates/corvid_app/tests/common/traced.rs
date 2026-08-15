//! Collecting this framework's own warnings, rather than printing them.
//!
//! The seam against the tests that use it is that nothing here asserts
//! anything: it installs a subscriber, hands back what was recorded, and holds
//! the one lock that keeps two tests in a binary from racing over `tracing`'s
//! per-callsite interest cache.

use std::{
    fmt,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

/// One event, as a subscriber saw it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Recorded {
    /// The callsite's name.
    pub(crate) name: String,
    /// Its level, as `tracing` prints it.
    pub(crate) level: String,
    /// Every field, in the order it arrived.
    pub(crate) fields: Vec<(String, String)>,
}

impl Recorded {
    /// What one field was recorded as.
    pub(crate) fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(recorded, _)| recorded == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Everything a subscriber collected.
#[derive(Default)]
pub(crate) struct Log {
    /// The warnings, in order. Spans are not collected: this crate opens none.
    pub(crate) events: Mutex<Vec<Recorded>>,
}

impl Log {
    pub(crate) fn events(&self) -> Vec<Recorded> {
        lock(&self.events).clone()
    }
}

/// The lock, with poisoning ignored, so a panic in one test reports as that
/// test failing rather than as every later one failing to read the log.
fn lock<T>(what: &Mutex<T>) -> MutexGuard<'_, T> {
    what.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A subscriber that records rather than prints.
struct Recorder(Arc<Log>);

impl Subscriber for Recorder {
    /// Warnings and above, and nothing else.
    ///
    /// The `dev` feature leaves a `DEBUG` event on every tick it discards a
    /// scratch on, and collecting those would make every assertion below about
    /// which build this is. What these tests are about is that a request this
    /// runtime cannot serve is loud, and "loud" means a level somebody's
    /// subscriber is filtered at.
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() <= tracing::Level::WARN
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

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

/// Collects an event's fields as text.
#[derive(Default)]
struct Fields(Vec<(String, String)>);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }

    /// Strings without the quotes a `Debug` would add.
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }
}

/// Held for the length of any test that emits a warning, whether or not it
/// collects one.
///
/// `tracing` caches, per callsite, whether anybody is interested in it, and the
/// cache is global to the process while a recording subscriber is thread-local.
/// So two tests in this binary that reach the same `warn!` -- one under
/// [`traced`] and one not -- race to write that cache, and the run where the
/// unsubscribed one wins is a run where the subscribed one collects nothing.
/// It was seen once, in a cold `--release` run of the whole workspace suite,
/// and never in this binary run on its own -- and when it fails it fails in the
/// collecting test, which is a test failing for a reason that has nothing to do
/// with what it names.
///
/// Serializing them is the fix rather than rebuilding the interest cache,
/// because a rebuild narrows the window and does not close it: the losing write
/// can land after it.
static WARNINGS: Mutex<()> = Mutex::new(());

/// Takes that lock, ignoring a poisoning for the reason [`lock`] does.
pub(crate) fn one_warning_at_a_time() -> MutexGuard<'static, ()> {
    lock(&WARNINGS)
}

/// Runs `body` with a recording subscriber installed on this thread, and with
/// no other test in this binary emitting a warning while it does.
pub(crate) fn traced(body: impl FnOnce()) -> Arc<Log> {
    let held = one_warning_at_a_time();
    let log = Arc::new(Log::default());
    tracing::subscriber::with_default(Recorder(Arc::clone(&log)), body);
    drop(held);
    log
}
