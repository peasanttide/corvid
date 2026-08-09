//! The observing end.

use std::fmt;
use std::sync::{Arc, PoisonError};

use super::Shared;
use crate::Seen;

/// The observing end of a signal.
///
/// Cheap to clone, and every clone observes the same cell independently: each
/// consumer keeps its own [`Seen`], so one that polls once a frame and one that
/// polls once a second neither block nor skip each other.
///
/// Every read hands back an [`Arc<T>`](Arc) rather than a `T`. That is what
/// keeps a consumer off the publishing path: a reader that wanted its own `T`
/// would have to clone one, and the only place a clone can be taken from a
/// shared cell consistently is under the lock. Deref for reading, and
/// `(*value).clone()` on the rare occasion a consumer needs to own and edit
/// one.
///
/// `Send + Sync` exactly when `T: Send + Sync`. The README says why, and checks
/// both directions.
pub struct Watch<T> {
    pub(super) shared: Arc<Shared<T>>,
}

impl<T> Clone for Watch<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

/// Prints the label and not the value, for the reason [`Emitter`](crate::Emitter)'s does.
impl<T> fmt::Debug for Watch<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Watch")
            .field("signal", &self.shared.label)
            .finish_non_exhaustive()
    }
}

impl<T> Watch<T> {
    /// What this signal is called in a trace.
    #[must_use]
    pub fn label(&self) -> &'static str {
        self.shared.label
    }

    /// The value in the cell, whether or not it has changed.
    ///
    /// The lock is held for a reference-count bump and released. What comes
    /// back is the value as of that instant and stays that value however many
    /// publications land afterwards, which is what makes it a consistent
    /// snapshot rather than a struct assembled from two publications.
    ///
    /// It waits for the lock and for nothing else, and what waiting for the
    /// lock is worth depends on who has it. Behind another reader or a
    /// [`set`](crate::Emitter::set) it is a reference-count bump; behind a
    /// [`modify`](crate::Emitter::modify) that has to copy it is a whole `T::clone`,
    /// because that is the one call in this crate that runs a `T`'s own code
    /// while holding the lock a reader takes.
    #[must_use]
    pub fn get(&self) -> Arc<T> {
        Arc::clone(&self.shared.lock().value)
    }

    /// A [`Seen`] that has already read whatever is in the cell now.
    ///
    /// The opposite starting point to [`Seen::default()`](Default::default),
    /// and the one for a consumer that wants what happens from here on: a
    /// thread that has just read the state it is about to keep in step with,
    /// and would otherwise be handed it a second time.
    #[must_use]
    pub fn seen_now(&self) -> Seen {
        Seen::at(self.shared.lock().sequence)
    }

    /// The value, if it has been published since `seen` last read it, and
    /// [`None`] if it has not.
    ///
    /// Returns `Some` exactly once per publication a consumer has not caught up
    /// with -- not once per publication. Three publications between two polls
    /// are one `Some` carrying the third value; the first two are gone.
    ///
    /// Emits a `TRACE` event called `corvid_signal.observed` when it returns
    /// `Some`, carrying the signal's label and the sequence number. That is the
    /// far end of the handoff whose near end is [`set`](crate::Emitter::set)'s span, so
    /// a trace that records timestamps shows the interval between a publication
    /// and a consumer noticing it. Nothing here measures that interval; the two
    /// timestamps are what a trace is read for.
    ///
    /// Waits for nothing but the lock, and holds it for a comparison and a
    /// reference-count bump.
    pub fn changed_since(&self, seen: &mut Seen) -> Option<Arc<T>> {
        let state = self.shared.lock();
        if state.sequence == seen.sequence() {
            return None;
        }

        let sequence = state.sequence;
        let value = Arc::clone(&state.value);
        drop(state);

        *seen = Seen::at(sequence);
        tracing::trace!(
            name: "corvid_signal.observed",
            signal = self.shared.label,
            sequence,
        );
        Some(value)
    }

    /// Parks this thread until something is published, and returns it.
    ///
    /// Returns immediately, without parking, when `seen` is already behind --
    /// the sequence number is read under the same lock a publication takes, so
    /// a publication that landed between two calls is not slept through.
    ///
    /// One publication wakes every thread parked here, rather than one of them.
    /// A signal is state and every consumer wants the current state, so waking
    /// one and leaving seven asleep on a value they are all behind on would be
    /// a queue with extra steps.
    ///
    /// # What the caller owes
    ///
    /// This is for a thread with nothing else to do: one whose entire job is to
    /// react to a signal, blocking between reactions. **No frame-rate or
    /// tick-rate path may call it.** A frame loop that parks here is a frame
    /// loop whose pacing is set by whichever subsystem publishes next, and a
    /// tick loop that parks here has handed the fixed step to a producer that
    /// knows nothing about it -- the two paths that must own their own clock are
    /// exactly the two that must poll [`changed_since`](Self::changed_since)
    /// instead. Nothing here can tell which thread it is on and nothing here
    /// enforces this.
    ///
    /// When every [`Emitter`](crate::Emitter) for this signal has been dropped, this parks
    /// forever: the signature returns a value and there is none to invent, so
    /// there is nothing it could return instead. A thread that must be able to
    /// exit needs a way out that does not come through this call -- its own
    /// shutdown flag, and a last publication from whoever sets that flag to
    /// wake it.
    pub fn blocking_wait(&self, seen: &mut Seen) -> Arc<T> {
        let mut state = self.shared.lock();
        while state.sequence == seen.sequence() {
            state = self
                .shared
                .published
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }

        let sequence = state.sequence;
        let value = Arc::clone(&state.value);
        drop(state);

        *seen = Seen::at(sequence);
        tracing::trace!(
            name: "corvid_signal.observed",
            signal = self.shared.label,
            sequence,
        );
        value
    }
}
