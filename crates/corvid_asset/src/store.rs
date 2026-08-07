//! The registry: the cache, the requests, and eviction.

use std::any::{Any, TypeId};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use crate::handle::Slot;
use crate::load::{self, Job, Landed, Shared, Step};
use crate::{Handle, Lod, Missing, Progress, Source};
// `Malformed` moved to `corvid_files` and grew an optional path, so that a
// level that knows which file objected and a decoder that only knows what it
// objected to raise the same type. `Asset::decode` still names it unqualified.
use corvid_files::Malformed;

/// What a type must be to be loaded.
///
/// ```
/// use corvid_asset::{Asset, Lod, Malformed};
///
/// /// A level, as its author wrote it: one line per row.
/// #[derive(Debug, Default, PartialEq)]
/// struct Rooms(Vec<String>);
///
/// impl Asset for Rooms {
///     fn placeholder() -> Self {
///         Self::default()
///     }
///
///     fn decode(bytes: &[u8], _lod: Lod) -> Result<Self, Malformed> {
///         let text = str::from_utf8(bytes).map_err(|_| Malformed::new("not utf-8"))?;
///         Ok(Self(text.lines().map(str::to_owned).collect()))
///     }
/// }
///
/// assert_eq!(Rooms::decode(b"hall\ncellar", Lod::FINEST)?.0.len(), 2);
/// # Ok::<(), Malformed>(())
/// ```
pub trait Asset: Send + Sync + 'static {
    /// Its placeholder, available before anything is read.
    fn placeholder() -> Self;

    /// Decode. `lod` is which detail level these bytes are wanted at.
    ///
    /// One byte string covers the whole chain: the loader reads a path once and
    /// decodes it at every level this kind has, coarsest first. A kind with one
    /// level ignores the argument.
    ///
    /// # Errors
    ///
    /// [`Malformed`], for bytes that are not what this kind of asset is.
    fn decode(bytes: &[u8], lod: Lod) -> Result<Self, Malformed>
    where
        Self: Sized;

    /// How many detail levels this kind has. One means no LOD chain.
    #[must_use]
    fn levels() -> u8 {
        1
    }
}

/// Why an asset is not there.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Unavailable {
    /// Nothing at that path.
    Missing(Missing),
    /// Bytes that will not decode.
    Malformed(Malformed),
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(missing) => missing.fmt(f),
            Self::Malformed(malformed) => malformed.fmt(f),
        }
    }
}

impl core::error::Error for Unavailable {}

impl From<Missing> for Unavailable {
    #[inline]
    fn from(missing: Missing) -> Self {
        Self::Missing(missing)
    }
}

impl From<Malformed> for Unavailable {
    #[inline]
    fn from(malformed: Malformed) -> Self {
        Self::Malformed(malformed)
    }
}

/// What one [`evict`](Assets::evict) let go of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Evicted {
    /// How many assets were dropped.
    pub assets: u32,
    /// How many bytes were read for them.
    pub bytes: u64,
}

/// One asset's place in the cache: its type, and the path it came from.
///
/// The type is half of the key because two kinds of asset may perfectly well be
/// decoded from one file, and a cache that keyed on the path alone would hand
/// the second one the first one's slot.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Key {
    of: TypeId,
    path: String,
}

impl Key {
    pub(crate) fn new<T: Asset>(path: &str) -> Self {
        Self {
            of: TypeId::of::<T>(),
            path: path.to_owned(),
        }
    }
}

/// One entry of the cache.
struct Cached {
    slot: Arc<dyn Any + Send + Sync>,
    bytes: u64,
    /// Whether a request for it has yet been answered, either way.
    outstanding: bool,
}

/// One asset whose levels have been decoded and not yet installed.
struct Pending {
    key: Key,
    steps: VecDeque<Step>,
}

/// Everything one lock covers.
#[derive(Default)]
struct Inner {
    cache: BTreeMap<Key, Cached>,
    pending: Vec<Pending>,
    tally: Progress,
}

impl Inner {
    fn find<T: Asset>(&self, key: &Key) -> Option<Handle<T>> {
        let cached = self.cache.get(key)?;
        Arc::clone(&cached.slot)
            .downcast::<Slot<T>>()
            .ok()
            .map(Handle::from_slot)
    }

    /// Put a fresh slot in the cache and count the request.
    fn open<T: Asset>(&mut self, key: Key) -> Handle<T> {
        let slot = Arc::new(Slot::new(T::placeholder()));
        self.cache.insert(
            key,
            Cached {
                slot: Arc::clone(&slot) as Arc<dyn Any + Send + Sync>,
                bytes: 0,
                outstanding: true,
            },
        );
        self.tally.requested = self.tally.requested.saturating_add(1);
        Handle::from_slot(slot)
    }

    fn settle(&mut self, key: &Key, bytes: u64, resident: bool) {
        if let Some(cached) = self.cache.get_mut(key) {
            cached.bytes = bytes;
            cached.outstanding = false;
        }
        if resident {
            self.tally.resident = self.tally.resident.saturating_add(1);
        } else {
            self.tally.failed = self.tally.failed.saturating_add(1);
        }
    }
}

/// The registry. One per process; the runtime owns it.
///
/// ```
/// use corvid_asset::{Asset, Assets, Lod, Malformed, Memory};
///
/// #[derive(Debug, Default, PartialEq)]
/// struct Note(String);
///
/// impl Asset for Note {
///     fn placeholder() -> Self {
///         Self("…".to_owned())
///     }
///     fn decode(bytes: &[u8], _lod: Lod) -> Result<Self, Malformed> {
///         String::from_utf8(bytes.to_vec())
///             .map(Self)
///             .map_err(|_| Malformed::new("not utf-8"))
///     }
/// }
///
/// let mut memory = Memory::new();
/// memory.insert("hello", b"a note".to_vec());
///
/// let assets = Assets::new(Box::new(memory));
/// let note = assets.load_now::<Note>("hello")?;
///
/// assert!(note.is_resident());
/// assert_eq!(note.get().0, "a note");
/// assert!(assets.is_settled());
/// # Ok::<(), corvid_asset::Unavailable>(())
/// ```
pub struct Assets {
    shared: Arc<Shared>,
    inner: Mutex<Inner>,
    landings: Mutex<Receiver<Landed>>,
    worker: Option<JoinHandle<()>>,
}

impl Assets {
    /// A registry reading through `source`.
    ///
    /// One loader thread is spawned here. Where the operating system refuses
    /// one, loading falls back to [`poll`](Self::poll), which runs the queue on
    /// the thread that called it — slower, and never a process that cannot
    /// load.
    #[must_use]
    pub fn new(source: Box<dyn Source>) -> Self {
        let (posts, landings) = channel();
        let shared = Arc::new(Shared::new(Arc::from(source), posts));
        let loader = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("corvid_asset".to_owned())
            .spawn(move || load::work(&loader))
            .ok();
        Self {
            shared,
            inner: Mutex::new(Inner::default()),
            landings: Mutex::new(landings),
            worker,
        }
    }

    /// Request an asset.
    ///
    /// Returns immediately with a handle answering the placeholder; the loader
    /// fills it in and [`poll`](Self::poll) installs it. Asking twice for one
    /// path answers with two handles to one asset and reads the source once.
    pub fn load<T: Asset>(&self, path: &str) -> Handle<T> {
        let key = Key::new::<T>(path);
        let (handle, job) = {
            let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(handle) = inner.find::<T>(&key) {
                return handle;
            }
            let handle = inner.open::<T>(key.clone());
            drop(inner);
            let job = job::<T>(key, path, handle.slot());
            (handle, job)
        };
        self.shared.push(job);
        handle
    }

    /// Request it and wait. What a barrier tick uses.
    ///
    /// The read and the decode happen on the calling thread, so this answers
    /// without a [`poll`](Self::poll) and without the loader thread having got
    /// to it. A path already resident answers with the handle it has; one that
    /// failed before is tried again, so the caller gets the reason rather than
    /// a remembered one.
    ///
    /// # Errors
    ///
    /// [`Unavailable::Missing`] for a path the source has nothing under, and
    /// [`Unavailable::Malformed`] for bytes that will not decode.
    pub fn load_now<T: Asset>(&self, path: &str) -> Result<Handle<T>, Unavailable> {
        let key = Key::new::<T>(path);
        // Whether this call owns the request. A retry of one already counted
        // does the work again and leaves the tally alone, so a barrier that has
        // already lifted stays lifted.
        let (handle, owned) = {
            let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            match inner.find::<T>(&key) {
                Some(handle) => {
                    let outstanding = inner
                        .cache
                        .get(&key)
                        .is_some_and(|cached| cached.outstanding);
                    if !outstanding && !handle.is_failed() {
                        return Ok(handle);
                    }
                    (handle, outstanding)
                }
                None => (inner.open::<T>(key.clone()), true),
            }
        };

        let settle = |bytes, resident| {
            if owned {
                self.inner
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .settle(&key, bytes, resident);
            }
        };

        match decode::<T>(self.shared.source(), path, handle.slot()) {
            Ok((bytes, steps)) => {
                for step in steps {
                    (step.install)();
                }
                settle(bytes, true);
                Ok(handle)
            }
            Err(why) => {
                handle.slot().fail();
                settle(0, false);
                Err(why)
            }
        }
    }

    /// Whether every outstanding request is answered.
    ///
    /// What the runtime asks before it lifts a `Command::Load` barrier.
    #[must_use]
    #[inline]
    pub fn is_settled(&self) -> bool {
        self.progress().is_settled()
    }

    /// How far along, for a loading screen the game draws itself.
    #[must_use]
    #[inline]
    pub fn progress(&self) -> Progress {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .tally
    }

    /// How many assets the cache is holding.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cache
            .len()
    }

    /// Whether the cache is holding nothing.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drop everything nothing holds.
    ///
    /// Called when the runtime chooses, never from a tick. An asset a job is
    /// still decoding is held by that job and survives.
    pub fn evict(&self) -> Evicted {
        let mut evicted = Evicted::default();
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cache
            .retain(|_, cached| {
                if Arc::strong_count(&cached.slot) > 1 {
                    return true;
                }
                evicted.assets = evicted.assets.saturating_add(1);
                evicted.bytes = evicted.bytes.saturating_add(cached.bytes);
                false
            });
        evicted
    }

    /// Pump the loader. Called once per frame from the runtime.
    ///
    /// At most one level per asset is installed per call, so a chain of four
    /// promotes over four frames and a frame's work is bounded by how many
    /// assets are in flight rather than by how detailed they are.
    pub fn poll(&self) {
        self.drain_queue();
        let landings = self.collect();
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        for landed in landings {
            let outstanding = inner
                .cache
                .get(landed.key())
                .is_some_and(|cached| cached.outstanding);
            if !outstanding {
                continue;
            }
            match landed {
                Landed::Ready { key, bytes, steps } => {
                    if let Some(cached) = inner.cache.get_mut(&key) {
                        cached.bytes = bytes;
                    }
                    inner.pending.push(Pending { key, steps });
                }
                Landed::Failed { key, mark } => {
                    mark();
                    inner.settle(&key, 0, false);
                }
            }
        }

        let mut waiting = Vec::with_capacity(inner.pending.len());
        let mut arrived = Vec::new();
        for mut pending in std::mem::take(&mut inner.pending) {
            if let Some(step) = pending.steps.pop_front() {
                (step.install)();
            }
            if pending.steps.is_empty() {
                arrived.push(pending.key);
            } else {
                waiting.push(pending);
            }
        }
        inner.pending = waiting;
        for key in arrived {
            let bytes = inner.cache.get(&key).map_or(0, |cached| cached.bytes);
            inner.settle(&key, bytes, true);
        }
    }

    /// Where a process with no loader thread does the loading.
    fn drain_queue(&self) {
        if self.worker.is_some() {
            return;
        }
        while let Some(job) = self.shared.take() {
            if !self.shared.post(job(self.shared.source())) {
                break;
            }
        }
    }

    fn collect(&self) -> Vec<Landed> {
        let landings = self.landings.lock().unwrap_or_else(PoisonError::into_inner);
        landings.try_iter().collect()
    }
}

/// Reading and decoding one asset, as the loader thread will do it.
fn job<T: Asset>(key: Key, path: &str, slot: &Arc<Slot<T>>) -> Job {
    let path = path.to_owned();
    let slot = Arc::clone(slot);
    Box::new(move |source| match decode::<T>(source, &path, &slot) {
        Ok((bytes, steps)) => Landed::Ready { key, bytes, steps },
        Err(_) => Landed::Failed {
            key,
            mark: Box::new(move || slot.fail()),
        },
    })
}

/// Reads once, decodes every level, coarsest first.
fn decode<T: Asset>(
    source: &dyn Source,
    path: &str,
    slot: &Arc<Slot<T>>,
) -> Result<(u64, VecDeque<Step>), Unavailable> {
    let bytes = source.read(path)?;
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let levels = T::levels().max(1);
    let mut steps = VecDeque::with_capacity(usize::from(levels));
    for level in (0..levels).rev() {
        let lod = Lod(level);
        steps.push_back(load::step(Arc::clone(slot), T::decode(&bytes, lod)?, lod));
    }
    Ok((size, steps))
}

impl From<Box<dyn Source>> for Assets {
    #[inline]
    fn from(source: Box<dyn Source>) -> Self {
        Self::new(source)
    }
}

/// Prints what the registry is holding, not what is in it.
impl fmt::Debug for Assets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Assets")
            .field("progress", &self.progress())
            .field("cached", &self.len())
            .field("threaded", &self.worker.is_some())
            .finish_non_exhaustive()
    }
}

/// Closes the queue and waits for the loader thread.
///
/// A job holds a handle to the slot it is filling, so nothing installs into
/// freed memory and there is nothing to cancel — the thread finishes what it
/// took and stops.
impl Drop for Assets {
    fn drop(&mut self) {
        self.shared.close();
        if let Some(worker) = self.worker.take() {
            drop(worker.join());
        }
    }
}
