//! The reference-counted handle, and the slot behind it.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, PoisonError, RwLock};

use crate::Lod;

/// What a handle points at: the current best answer, and how good it is.
pub(crate) struct Slot<T> {
    value: RwLock<Arc<T>>,
    lod: AtomicU8,
    failed: AtomicBool,
}

impl<T> Slot<T> {
    /// A slot answering `placeholder` at [`Lod::PLACEHOLDER`].
    pub(crate) fn new(placeholder: T) -> Self {
        Self {
            value: RwLock::new(Arc::new(placeholder)),
            lod: AtomicU8::new(Lod::PLACEHOLDER.0),
            failed: AtomicBool::new(false),
        }
    }

    /// Put a decoded level in. The value goes in before the level does, so a
    /// reader never sees a level finer than what it can read.
    pub(crate) fn install(&self, value: T, lod: Lod) {
        *self.value.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(value);
        self.lod.store(lod.0, Ordering::Release);
    }

    /// Mark that nothing will ever land here.
    pub(crate) fn fail(&self) {
        self.failed.store(true, Ordering::Release);
    }

    fn current(&self) -> Arc<T> {
        Arc::clone(&self.value.read().unwrap_or_else(PoisonError::into_inner))
    }

    fn lod(&self) -> Lod {
        Lod(self.lod.load(Ordering::Acquire))
    }

    fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
}

/// A reference-counted handle to a loaded asset.
///
/// `Clone` and `Debug` and nothing else — **not** `Hash`, **not** `Eq`, **not**
/// `Serialize`. So it cannot satisfy [`corvid_behavior::Data`] and cannot be
/// put in a [`State`](corvid_behavior::State). That refusal is the
/// ring rule expressed as a type: an asset lives on one machine, and a state
/// that named one would be a state two peers could not compare.
///
/// ```compile_fail
/// fn only_data<T: corvid_behavior::Data>() {}
///
/// // `Handle` is missing `Serialize`, `Hash` and `Eq`, so this does not build.
/// only_data::<corvid_asset::Handle<u32>>();
/// ```
///
/// The same check on a type that *is* `Data` builds, which is what says the
/// check above is refusing the handle rather than the syntax:
///
/// ```
/// fn only_data<T: corvid_behavior::Data>() {}
///
/// only_data::<u32>();
/// ```
pub struct Handle<T> {
    slot: Arc<Slot<T>>,
}

impl<T> Handle<T> {
    pub(crate) const fn from_slot(slot: Arc<Slot<T>>) -> Self {
        Self { slot }
    }

    pub(crate) const fn slot(&self) -> &Arc<Slot<T>> {
        &self.slot
    }

    /// What is loaded, or the placeholder while it is not.
    ///
    /// Always an answer, never an [`Option`]: a renderer that had to branch on
    /// whether its mesh arrived would branch every frame for the whole life of
    /// the program to cover a case that lasts two hundred milliseconds.
    /// [`is_resident`](Self::is_resident) exists for the loading screen, which
    /// is the one caller that actually cares.
    ///
    /// An [`Arc`] rather than a `&T`, because a promotion replaces what the
    /// handle answers with and a borrow would have to hold the lock the loader
    /// takes to install it.
    #[must_use]
    #[inline]
    pub fn get(&self) -> Arc<T> {
        self.slot.current()
    }

    /// Whether [`get`](Self::get) is answering with something that was decoded
    /// rather than with the placeholder.
    #[must_use]
    #[inline]
    pub fn is_resident(&self) -> bool {
        !self.slot.lod().is_placeholder()
    }

    /// Whether this asset will never arrive.
    ///
    /// [`get`](Self::get) still answers, with the placeholder, so a failed load
    /// is a game that draws a grey box rather than a game that stops.
    #[must_use]
    #[inline]
    pub fn is_failed(&self) -> bool {
        self.slot.failed()
    }

    /// The level of detail [`get`](Self::get) is currently answering at.
    #[must_use]
    #[inline]
    pub fn lod(&self) -> Lod {
        self.slot.lod()
    }

    /// How many handles, this one included, are keeping the asset resident.
    #[must_use]
    #[inline]
    pub fn holders(&self) -> usize {
        Arc::strong_count(&self.slot)
    }

    /// A handle that does not keep the asset resident.
    #[must_use]
    #[inline]
    pub fn downgrade(&self) -> Weak<T> {
        Weak {
            slot: Arc::downgrade(&self.slot),
        }
    }
}

/// Refcount only. Cloning a handle does not clone the asset.
impl<T> Clone for Handle<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            slot: Arc::clone(&self.slot),
        }
    }
}

/// Prints where the handle is, not what is in it.
///
/// A loaded asset is arbitrarily large and is not required to be `Debug` at
/// all, so this reports the two things a log line wants: whether the real thing
/// has arrived and at which level.
impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handle")
            .field("lod", &self.lod())
            .field("resident", &self.is_resident())
            .field("failed", &self.is_failed())
            .finish_non_exhaustive()
    }
}

impl<T> From<Handle<T>> for Weak<T> {
    #[inline]
    fn from(handle: Handle<T>) -> Self {
        handle.downgrade()
    }
}

/// A handle that does not keep the asset resident.
///
/// What a cache of "the last thing the cursor hovered" holds: it says which
/// asset without being a reason to keep it in memory.
pub struct Weak<T> {
    slot: std::sync::Weak<Slot<T>>,
}

impl<T> Weak<T> {
    /// A handle again, or [`None`] if nothing else was holding it.
    #[must_use]
    #[inline]
    pub fn upgrade(&self) -> Option<Handle<T>> {
        self.slot.upgrade().map(Handle::from_slot)
    }
}

impl<T> Clone for Weak<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            slot: self.slot.clone(),
        }
    }
}

impl<T> fmt::Debug for Weak<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Weak")
            .field("live", &(self.slot.strong_count() > 0))
            .finish_non_exhaustive()
    }
}

impl<T> TryFrom<Weak<T>> for Handle<T> {
    type Error = Gone;

    /// # Errors
    ///
    /// [`Gone`], when nothing else was holding the asset.
    fn try_from(weak: Weak<T>) -> Result<Self, Gone> {
        weak.upgrade().ok_or(Gone)
    }
}

/// The asset a [`Weak`] named is no longer resident.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Gone;

impl fmt::Display for Gone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the asset is no longer resident")
    }
}

impl core::error::Error for Gone {}
