//! The background loader: one thread, one queue, and one channel back.

use std::collections::VecDeque;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use crate::store::Key;
use crate::{Lod, Source};

/// How far along a set of requests is.
///
/// ```
/// use corvid_asset::Progress;
///
/// let none = Progress::default();
/// assert!(none.is_settled());
///
/// let loading = Progress { requested: 3, resident: 1, failed: 0 };
/// assert!(!loading.is_settled());
/// assert_eq!(loading.outstanding(), 2);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Progress {
    /// How many assets have been asked for.
    pub requested: u32,
    /// How many have arrived.
    pub resident: u32,
    /// How many will not.
    pub failed: u32,
}

impl Progress {
    /// How many requests have neither arrived nor failed.
    #[must_use]
    #[inline]
    pub const fn outstanding(self) -> u32 {
        self.requested
            .saturating_sub(self.resident.saturating_add(self.failed))
    }

    /// Whether every request has been answered, one way or the other.
    #[must_use]
    #[inline]
    pub const fn is_settled(self) -> bool {
        self.outstanding() == 0
    }
}

/// Putting one decoded level into the slot it belongs to.
pub(crate) type Install = Box<dyn FnOnce() + Send>;

/// Reading and decoding one asset, off the frame thread.
pub(crate) type Job = Box<dyn FnOnce(&dyn Source) -> Landed + Send>;

/// One level of one asset, decoded and waiting to be installed.
pub(crate) struct Step {
    pub(crate) install: Install,
}

/// What the loader hands back.
pub(crate) enum Landed {
    /// Every level decoded, coarsest first.
    Ready {
        key: Key,
        bytes: u64,
        steps: VecDeque<Step>,
    },
    /// Nothing will arrive.
    Failed { key: Key, mark: Install },
}

impl Landed {
    pub(crate) const fn key(&self) -> &Key {
        match self {
            Self::Ready { key, .. } | Self::Failed { key, .. } => key,
        }
    }
}

/// The jobs waiting, and whether any more are coming.
struct Queue {
    jobs: VecDeque<Job>,
    closed: bool,
}

/// What the frame thread and the loader thread both hold.
pub(crate) struct Shared {
    source: Arc<dyn Source>,
    queue: Mutex<Queue>,
    wake: Condvar,
    posts: Sender<Landed>,
}

impl Shared {
    pub(crate) fn new(source: Arc<dyn Source>, posts: Sender<Landed>) -> Self {
        Self {
            source,
            queue: Mutex::new(Queue {
                jobs: VecDeque::new(),
                closed: false,
            }),
            wake: Condvar::new(),
            posts,
        }
    }

    pub(crate) fn source(&self) -> &dyn Source {
        self.source.as_ref()
    }

    pub(crate) fn push(&self, job: Job) {
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .jobs
            .push_back(job);
        self.wake.notify_one();
    }

    /// One job if there is one, without waiting. What a process with no loader
    /// thread drains from [`poll`](crate::Assets::poll).
    pub(crate) fn take(&self) -> Option<Job> {
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .jobs
            .pop_front()
    }

    pub(crate) fn post(&self, landed: Landed) -> bool {
        self.posts.send(landed).is_ok()
    }

    /// Stop the loader thread after it finishes what it is holding.
    pub(crate) fn close(&self) {
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .closed = true;
        self.wake.notify_all();
    }

    /// The next job, waiting for one, or [`None`] once the queue is closed and
    /// empty.
    fn next(&self) -> Option<Job> {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(job) = queue.jobs.pop_front() {
                return Some(job);
            }
            if queue.closed {
                return None;
            }
            queue = self
                .wake
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// The loader thread's whole body.
pub(crate) fn work(shared: &Shared) {
    while let Some(job) = shared.next() {
        if !shared.post(job(shared.source())) {
            break;
        }
    }
}

/// A decoded level, ready to be installed when the frame thread next polls.
pub(crate) fn step<T: Send + Sync + 'static>(
    slot: Arc<crate::handle::Slot<T>>,
    value: T,
    lod: Lod,
) -> Step {
    Step {
        install: Box::new(move || slot.install(value, lod)),
    }
}
