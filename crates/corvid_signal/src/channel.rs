//! The cell, and the two handles onto it.

use std::{
    fmt, mem,
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
};

use crate::Seen;

/// The sequence number the cell's initial value carries.
///
/// One rather than zero, so that `Seen::default()` — which is zero — has not
/// seen it. A consumer's first poll then reports the state that was already
/// there, which is the whole reason a latest-value cell is not a queue.
const FIRST: u64 = 1;

/// What the two handles share: the value, the sequence number that says which
/// publication it is, and the condition every blocked consumer waits on.
struct Shared<T> {
    /// What a span calls this signal. `&'static str` because it names a signal
    /// in a program rather than a value in a session, so there is nothing to
    /// build one out of at runtime and nothing to allocate.
    label: &'static str,
    /// The cell. A `Mutex` and not a lock-free construction: the pointer and
    /// the sequence number have to move together, and there is no width to
    /// exchange sixteen bytes at that every platform this targets has.
    state: Mutex<State<T>>,
    /// Signalled after every publication, with the lock released first so a
    /// woken consumer does not wake into the lock the publisher still holds.
    published: Condvar,
}

/// The value, and which publication it is.
struct State<T> {
    /// The latest value. There is no older one anywhere; that is the type.
    ///
    /// Behind an `Arc` so that everything done under the lock is a pointer
    /// move: a publication swaps this and a consumer bumps a reference count,
    /// and neither one runs a line of the caller's code while holding it. The
    /// `T` itself is then read outside the lock by whoever wanted it.
    value: Arc<T>,
    /// Bumped by every publication. Consumers compare it against their [`Seen`]
    /// rather than comparing values, so a `T` that is not `PartialEq` — or one
    /// whose equality is expensive — still works, and so does a publication
    /// that happened to write the value that was already there.
    ///
    /// The bump wraps rather than checks. Overflow checks are on in the dev
    /// profile, so a plain `+ 1` is a signal that panics on its own counter,
    /// and a panic nobody could have prevented is the worst kind. What wrapping
    /// costs is the invariant `FIRST` is there to set up: the publication after
    /// `u64::MAX` is numbered zero, which is what a `Seen::default()` says it
    /// has already read, so one consumer that had never polled would miss one
    /// publication. At a publication every nanosecond that is five hundred and
    /// eighty years out, and the saturating alternative is worse — a counter
    /// that stopped moving would stop reporting changes for everybody, for
    /// good.
    sequence: u64,
}

impl<T> Shared<T> {
    /// The lock, with poisoning ignored.
    ///
    /// A poisoned mutex here means a [`modify`](Emitter::modify) closure — or
    /// the `T::clone` its copy-on-write step may take first, or in the one race
    /// that page describes, the `T::drop` that race lets in — panicked while
    /// the lock was held. Those are the only caller code that runs under it at
    /// all; a [`set`](Emitter::set)'s allocation, the drop of the value a `set`
    /// replaced and a consumer's own copy of what it read all happen outside.
    /// Propagating that would mean a panicking `unwrap`, which the workspace
    /// denies, and refusing to serve would mean a signal that stops carrying a
    /// window size because something unrelated panicked once. So the value is
    /// served as it stands, and what "as it stands" means is the caller's
    /// business: see the note on [`modify`](Emitter::modify).
    fn lock(&self) -> MutexGuard<'_, State<T>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Opens a signal, and hands back the two ends of it.
///
/// The `label` is what a span calls this signal in a trace, and it is a
/// parameter rather than something optional because a span named after nothing
/// is a span nobody can read: a trace of six subsystems publishing state has to
/// say *which* of them published. Give it the name the state has in the program
/// — `"surface"`, `"peers"`, `"audio devices"` — rather than the type's name,
/// since two signals may well carry the same type.
///
/// The initial value is a value and not a `None`. A consumer reading this
/// signal before anything has been published gets state rather than an absence,
/// which is what removes the branch every consumer would otherwise write.
///
/// ```
/// use corvid_signal::{Seen, channel};
///
/// let (emit, watch) = channel("surface", (1280_u32, 720_u32));
/// let mut seen = Seen::default();
///
/// // The value that was already there is a change to a consumer that has seen
/// // nothing, so a subsystem starting up mid-session is told the current size.
/// // What comes back is an `Arc<T>`, which is why these compare through `*`.
/// assert_eq!(watch.changed_since(&mut seen).as_deref(), Some(&(1280, 720)));
/// assert_eq!(watch.changed_since(&mut seen).as_deref(), None);
///
/// emit.set((1920, 1080));
/// assert_eq!(watch.changed_since(&mut seen).as_deref(), Some(&(1920, 1080)));
/// ```
#[must_use]
pub fn channel<T>(label: &'static str, initial: T) -> (Emitter<T>, Watch<T>) {
    let shared = Arc::new(Shared {
        label,
        state: Mutex::new(State {
            value: Arc::new(initial),
            sequence: FIRST,
        }),
        published: Condvar::new(),
    });

    (
        Emitter {
            shared: Arc::clone(&shared),
        },
        Watch { shared },
    )
}

/// The publishing end of a signal.
///
/// Cheap to clone, and every clone publishes to the same cell — a subsystem
/// that has two threads reporting the same state hands one to each. Publishing
/// never waits for a consumer and never queues, so a value published while
/// nobody is looking is the value the next consumer to look will see, and every
/// value published between two polls is dropped.
///
/// `Send + Sync` exactly when `T: Send + Sync`. The README says why the value
/// living behind an `Arc` is what makes `Sync` part of that, and checks both
/// directions.
pub struct Emitter<T> {
    shared: Arc<Shared<T>>,
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
/// — the first of which already holds it.
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
    /// ends. Freeing a large `T` — a device list is four hundred thousand
    /// deallocations — is most of what a `set` costs by the clock and none of
    /// what it costs anybody else, because it happens with the lock released
    /// and nothing waiting on it. A span that counted it would report a
    /// publisher's own bookkeeping as time some consumer spent behind this
    /// signal, which is the one thing a trace of this crate is read to find.
    ///
    /// # What runs under the lock
    ///
    /// A pointer swap and an integer increment, and nothing else. The `Arc` is
    /// built before the lock is taken and the value it replaced is dropped after
    /// the lock is released, so no line of a `T`'s own code — no allocation, no
    /// `Drop`, no `Clone` — runs while a consumer could be waiting on it. That
    /// is what makes "a publication never waits for a consumer" a statement
    /// about the implementation rather than about the condition variable alone.
    ///
    /// Dropping outside the lock also makes one re-entrant path work: a `T`
    /// whose `Drop` publishes to this same signal — a value that owns an
    /// [`Emitter`] and reports its own retirement — does not deadlock. That is
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
    /// consumer is still holding the value about to be edited — that consumer
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
    /// take first, so everything waiting on this signal — every other
    /// publication, every [`get`](Watch::get), every
    /// [`changed_since`](Watch::changed_since) — waits for both. Keep `f` to the
    /// edit. Touching this signal from inside `f`, by any handle, deadlocks.
    ///
    /// One more thing can run under that lock, and no caller chooses when.
    /// `Arc::make_mut` lets go of the reference it copied away from, and if the
    /// consumer holding the other one let go in the same instant, that release
    /// is the last and the old `T` is dropped where it stands — inside the
    /// lock. [`set`](Self::set) drops what it replaced after releasing the lock
    /// and says so; `modify` cannot promise the same. So the one re-entrant
    /// shape `set` is written to survive — a `T` whose `Drop` publishes to this
    /// signal — is not a shape `modify` survives, and it deadlocks on a race
    /// rather than every time, which is the worse of the two ways to find out.
    /// Such a `T` is published with `set` and edited nowhere.
    ///
    /// A publication happens whether or not `f` changed anything, because
    /// nothing here can tell: `T` is not required to be `PartialEq`, and a
    /// signal that only woke consumers for edits it could prove were edits
    /// would be a signal whose behaviour depended on the game's `PartialEq`.
    ///
    /// If `f` panics, the edit it had made so far stays in the cell and is
    /// **not** published — the sequence number is bumped after `f` returns, so
    /// consumers polling [`changed_since`](Watch::changed_since) are not told,
    /// while [`get`](Watch::get) returns the half-edited value. A consumer that
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
    shared: Arc<Shared<T>>,
}

impl<T> Clone for Watch<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

/// Prints the label and not the value, for the reason [`Emitter`]'s does.
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
    /// [`set`](Emitter::set) it is a reference-count bump; behind a
    /// [`modify`](Emitter::modify) that has to copy it is a whole `T::clone`,
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
    /// with — not once per publication. Three publications between two polls
    /// are one `Some` carrying the third value; the first two are gone.
    ///
    /// Emits a `TRACE` event called `corvid_signal.observed` when it returns
    /// `Some`, carrying the signal's label and the sequence number. That is the
    /// far end of the handoff whose near end is [`set`](Emitter::set)'s span, so
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
    /// Returns immediately, without parking, when `seen` is already behind —
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
    /// knows nothing about it — the two paths that must own their own clock are
    /// exactly the two that must poll [`changed_since`](Self::changed_since)
    /// instead. Nothing here can tell which thread it is on and nothing here
    /// enforces this.
    ///
    /// When every [`Emitter`] for this signal has been dropped, this parks
    /// forever: the signature returns a value and there is none to invent, so
    /// there is nothing it could return instead. A thread that must be able to
    /// exit needs a way out that does not come through this call — its own
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
