//! The cell, and the two handles onto it.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

/// The sequence number the cell's initial value carries.
///
/// One rather than zero, so that `Seen::default()` -- which is zero -- has not
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
    /// rather than comparing values, so a `T` that is not `PartialEq` -- or one
    /// whose equality is expensive -- still works, and so does a publication
    /// that happened to write the value that was already there.
    ///
    /// The bump wraps rather than checks. Overflow checks are on in the dev
    /// profile, so a plain `+ 1` is a signal that panics on its own counter,
    /// and a panic nobody could have prevented is the worst kind. What wrapping
    /// costs is the invariant `FIRST` is there to set up: the publication after
    /// `u64::MAX` is numbered zero, which is what a `Seen::default()` says it
    /// has already read, so one consumer that had never polled would miss one
    /// publication. At a publication every nanosecond that is five hundred and
    /// eighty years out, and the saturating alternative is worse -- a counter
    /// that stopped moving would stop reporting changes for everybody, for
    /// good.
    sequence: u64,
}

impl<T> Shared<T> {
    /// The lock, with poisoning ignored.
    ///
    /// A poisoned mutex here means a [`modify`](Emitter::modify) closure -- or
    /// the `T::clone` its copy-on-write step may take first, or in the one race
    /// that page describes, the `T::drop` that race lets in -- panicked while
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
/// -- `"surface"`, `"peers"`, `"audio devices"` -- rather than the type's name,
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

mod emitter;
mod watch;

pub use emitter::Emitter;
pub use watch::Watch;
