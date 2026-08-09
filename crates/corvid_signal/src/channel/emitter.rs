//! The publishing end.

use std::sync::Arc;
use std::{fmt, mem};

use super::Shared;

/// The publishing end of a signal.
///
/// Cheap to clone, and every clone publishes to the same cell -- a subsystem
/// that has two threads reporting the same state hands one to each. Publishing
/// never waits for a consumer and never queues, so a value published while
/// nobody is looking is the value the next consumer to look will see, and every
/// value published between two polls is dropped.
///
/// `Send + Sync` exactly when `T: Send + Sync`. The README says why the value
/// living behind an `Arc` is what makes `Sync` part of that, and checks both
/// directions.
pub struct Emitter<T> {
    pub(super) shared: Arc<Shared<T>>,
}

impl<T> Clone for Emitter<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

/// Prints the label and not the value.
///
/// Reading the value would mean taking the lock, and the two places a `Debug`
/// is most likely to be called from are a `modify` closure and a panic message
/// -- the first of which already holds it.
impl<T> fmt::Debug for Emitter<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Emitter")
            .field("signal", &self.shared.label)
            .finish_non_exhaustive()
    }
}

impl<T> Emitter<T> {
    /// What this signal is called in a trace.
    #[must_use]
    pub fn label(&self) -> &'static str {
        self.shared.label
    }

    /// Publishes a value, replacing whatever was there.
    ///
    /// Opens a `DEBUG` span called `corvid_signal.set`, with the signal's label
    /// and the sequence number this publication was given as fields. The span
    /// covers the allocation, the lock, the store and the wakeup, so what it
    /// times is the handoff itself including whatever it queued behind.
    ///
    /// It is left *before* the value this publication replaced is dropped, and
    /// that omission is deliberate rather than an accident of where a local
    /// ends. Freeing a large `T` -- a device list is four hundred thousand
    /// deallocations -- is most of what a `set` costs by the clock and none of
    /// what it costs anybody else, because it happens with the lock released
    /// and nothing waiting on it. A span that counted it would report a
    /// publisher's own bookkeeping as time some consumer spent behind this
    /// signal, which is the one thing a trace of this crate is read to find.
    ///
    /// # What runs under the lock
    ///
    /// A pointer swap and an integer increment, and nothing else. The `Arc` is
    /// built before the lock is taken and the value it replaced is dropped after
    /// the lock is released, so no line of a `T`'s own code -- no allocation, no
    /// `Drop`, no `Clone` -- runs while a consumer could be waiting on it. That
    /// is what makes "a publication never waits for a consumer" a statement
    /// about the implementation rather than about the condition variable alone.
    ///
    /// Dropping outside the lock also makes one re-entrant path work: a `T`
    /// whose `Drop` publishes to this same signal -- a value that owns an
    /// [`Emitter`] and reports its own retirement -- does not deadlock. That is
    /// one path made to work rather than a general promise. The
    /// [`modify`](Self::modify) closure still runs *under* the lock, and
    /// touching this signal from inside it deadlocks; `std`'s `Mutex` is not
    /// reentrant and nothing here makes it one.
    ///
    /// ```
    /// use corvid_signal::{Seen, channel};
    ///
    /// let (emit, watch) = channel("peers", 0_u32);
    /// let mut seen = watch.seen_now();
    ///
    /// // Three publications, one observation: the two in the middle are gone,
    /// // and no consumer can tell they happened.
    /// emit.set(1);
    /// emit.set(2);
    /// emit.set(3);
    /// assert_eq!(watch.changed_since(&mut seen).as_deref(), Some(&3));
    /// assert_eq!(watch.changed_since(&mut seen).as_deref(), None);
    /// ```
    pub fn set(&self, value: T) {
        let span = tracing::debug_span!(
            "corvid_signal.set",
            signal = self.shared.label,
            sequence = tracing::field::Empty,
        );
        let entered = span.enter();

        // Allocated here, on purpose: this is the one part of a publication
        // that can take an unbounded amount of time, and it is outside the
        // lock.
        let value = Arc::new(value);

        let mut state = self.shared.lock();
        let previous = mem::replace(&mut state.value, value);
        state.sequence = state.sequence.wrapping_add(1);
        let sequence = state.sequence;
        drop(state);

        self.shared.published.notify_all();
        span.record("sequence", sequence);
        drop(entered);

        // Explicit, and load-bearing: `previous` would otherwise be dropped at
        // the end of this function, which is still after `state` was released,
        // but only because of the order two locals happen to be declared in.
        // It drops the `T` only when no consumer is still holding it.
        drop(previous);
    }
}

impl<T: Clone> Emitter<T> {
    /// Edits the value in place and publishes the result.
    ///
    /// Opens a `DEBUG` span called `corvid_signal.modify`, carrying the same
    /// fields as [`set`](Self::set)'s.
    ///
    /// # What it costs
    ///
    /// This is copy-on-write, and which of the two words applies depends on
    /// what the consumers are doing rather than on what the caller wrote.
    /// `Arc::make_mut` edits the value where it lies when this emitter's cell
    /// holds the only reference to it, and clones the whole `T` first when a
    /// consumer is still holding the value about to be edited -- that consumer
    /// has a snapshot, and a snapshot that changed underneath its reader would
    /// be worse than a copy.
    ///
    /// So a device list that gained one entry is one push on a signal nobody is
    /// reading right now, and a push plus a copy of the list on one that
    /// somebody is. The copy is bounded by one per publication however many
    /// consumers there are, and it never happens twice for one edit. Against
    /// clone-edit-[`set`](Self::set), which copies unconditionally, this is at
    /// worst the same work.
    ///
    /// # What the caller owes
    ///
    /// `f` runs with the lock held, and so does the clone `Arc::make_mut` may
    /// take first, so everything waiting on this signal -- every other
    /// publication, every [`get`](crate::Watch::get), every
    /// [`changed_since`](crate::Watch::changed_since) -- waits for both. Keep `f` to the
    /// edit. Touching this signal from inside `f`, by any handle, deadlocks.
    ///
    /// One more thing can run under that lock, and no caller chooses when.
    /// `Arc::make_mut` lets go of the reference it copied away from, and if the
    /// consumer holding the other one let go in the same instant, that release
    /// is the last and the old `T` is dropped where it stands -- inside the
    /// lock. [`set`](Self::set) drops what it replaced after releasing the lock
    /// and says so; `modify` cannot promise the same. So the one re-entrant
    /// shape `set` is written to survive -- a `T` whose `Drop` publishes to this
    /// signal -- is not a shape `modify` survives, and it deadlocks on a race
    /// rather than every time, which is the worse of the two ways to find out.
    /// Such a `T` is published with `set` and edited nowhere.
    ///
    /// A publication happens whether or not `f` changed anything, because
    /// nothing here can tell: `T` is not required to be `PartialEq`, and a
    /// signal that only woke consumers for edits it could prove were edits
    /// would be a signal whose behaviour depended on the game's `PartialEq`.
    ///
    /// If `f` panics, the edit it had made so far stays in the cell and is
    /// **not** published -- the sequence number is bumped after `f` returns, so
    /// consumers polling [`changed_since`](crate::Watch::changed_since) are not told,
    /// while [`get`](crate::Watch::get) returns the half-edited value. A consumer that
    /// was holding the previous value still holds the previous value, because
    /// the copy-on-write step ran before `f` did. A `T` whose intermediate
    /// states are not values the rest of the program can read should be edited
    /// on a clone and published with [`set`](Self::set).
    ///
    /// ```
    /// use corvid_signal::{Seen, channel};
    ///
    /// let (emit, watch) = channel("audio devices", vec!["built-in"]);
    /// let mut seen = watch.seen_now();
    ///
    /// emit.modify(|devices| devices.push("headset"));
    /// assert_eq!(
    ///     watch.changed_since(&mut seen).as_deref(),
    ///     Some(&vec!["built-in", "headset"]),
    /// );
    /// ```
    pub fn modify(&self, f: impl FnOnce(&mut T)) {
        let span = tracing::debug_span!(
            "corvid_signal.modify",
            signal = self.shared.label,
            sequence = tracing::field::Empty,
        );
        let entered = span.enter();

        let mut state = self.shared.lock();
        // The copy-on-write step. Free when nobody else is holding this value,
        // and one `T::clone` when somebody is.
        f(Arc::make_mut(&mut state.value));
        state.sequence = state.sequence.wrapping_add(1);
        let sequence = state.sequence;
        drop(state);

        self.shared.published.notify_all();
        span.record("sequence", sequence);
        drop(entered);
    }
}
