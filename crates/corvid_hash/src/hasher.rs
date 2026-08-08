//! The digest and the streaming state that produces it.

use core::fmt;
use core::hash::Hash;

use crate::mix::mix;

/// The seed the chain starts from. Any non-zero value works; this one is the
/// fractional part of the golden ratio, which has no structure to interact with
/// the mixer.
const SEED: u64 = 0x9e37_79b9_7f4a_7c15;

/// Sixty-four bits standing for the whole of whatever was hashed.
///
/// A mark is exchanged every tick by every peer, so the width is chosen for the
/// wire rather than for cryptography: a false agreement at `2^-64` per tick is
/// far below the rate at which anything else in the stack fails, and nothing
/// here resists an adversary who is choosing the inputs.
///
/// ```
/// # use corvid_hash::Digest;
/// assert_eq!(Digest::from_u64(0xdead_beef).to_string(), "00000000deadbeef");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct Digest(u64);

impl Digest {
    /// The absence of a digest, which is not the digest of the empty input.
    ///
    /// ```
    /// # use corvid_hash::{Digest, digest};
    /// assert_ne!(digest(&()), Digest::ZERO);
    /// ```
    pub const ZERO: Self = Self(0);

    /// Wraps raw bits.
    #[must_use]
    pub const fn from_u64(bits: u64) -> Self {
        Self(bits)
    }

    /// Unwraps the raw bits.
    #[must_use]
    pub const fn to_u64(self) -> u64 {
        self.0
    }
}

impl From<u64> for Digest {
    #[inline]
    fn from(bits: u64) -> Self {
        Self(bits)
    }
}

impl From<Digest> for u64 {
    #[inline]
    fn from(digest: Digest) -> Self {
        digest.0
    }
}

/// Sixteen lowercase hex digits and nothing else, so a digest pastes into a log
/// line, a file name or a golden table without quoting.
impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest(0x{:016x})", self.0)
    }
}

/// The streaming state a digest is accumulated in, and a
/// [`core::hash::Hasher`] whose answer is the same on every target.
///
/// ```
/// use core::hash::Hash;
///
/// #[derive(Hash)]
/// struct Ship { hull: i16, shield: u16 }
///
/// let mut hasher = corvid_hash::Hasher::new();
/// Ship { hull: 3, shield: 4 }.hash(&mut hasher);
/// assert_eq!(hasher.digest(), corvid_hash::digest(&Ship { hull: 3, shield: 4 }));
/// ```
///
/// # Why this is a `Hasher` rather than a trait of its own
///
/// So that `#[derive(Hash)]` is the derive, and every type in `core` and
/// `alloc` that already implements [`Hash`] is already hashable here.
///
/// The two properties a shared digest needs are not [`Hash`]'s to provide, and
/// both are this type's.
///
/// **A fixed key.** [`new`](Self::new) seeds from a constant, where
/// `std::collections::hash_map::DefaultHasher` seeds from the process.
///
/// **A fixed width for every write.** The default methods on
/// [`core::hash::Hasher`] forward to [`write`](core::hash::Hasher::write) in
/// *native* endian, and `write_usize` is as wide as the target's pointer — so a
/// `Vec`'s length prefix absorbs four bytes on `wasm32` and eight on `x86_64`,
/// and a browser peer desyncs from a native one on the first tick. Every
/// `write_*` here is overridden: integers absorb little-endian at their declared
/// width, and `usize` and `isize` absorb as 64 bits whatever the target's
/// pointer is.
///
/// # What the overrides do not reach
///
/// [`Hash::hash_slice`], which `core` implements for every primitive integer by
/// reinterpreting the whole slice as bytes and calling
/// [`write`](core::hash::Hasher::write) once. A `Vec<u32>` or an `[i16; 4]`
/// therefore absorbs raw bytes in the target's own order, past every override
/// below, and no call site can see it happening:
///
/// ```
/// use core::hash::Hasher as _;
///
/// let mut raw = corvid_hash::Hasher::new();
/// raw.write_usize(2);
/// raw.write(&[0x01, 0x00, 0x02, 0x00]);
/// assert_eq!(corvid_hash::digest(&[1_u16, 2][..]), raw.digest());
/// ```
///
/// There is no fix here — `write` is handed bytes and is not told what they
/// were — so the crate does not build for a big-endian target. `lib.rs` carries
/// that refusal and says why.
///
/// ```
/// use core::hash::Hasher as _;
///
/// let mut narrow = corvid_hash::Hasher::new();
/// narrow.write_u64(7);
/// let mut wide = corvid_hash::Hasher::new();
/// wide.write_usize(7);
/// assert_eq!(narrow.digest(), wide.digest());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Hasher {
    state: u64,
    len: u64,
}

impl Hasher {
    /// Starts a chain from the shared seed.
    #[must_use]
    pub const fn new() -> Self {
        Self::with_seed(SEED)
    }

    /// Starts a chain from a seed of your own, for domain separation.
    ///
    /// ```
    /// # use corvid_hash::Hasher;
    /// assert_ne!(
    ///     Hasher::new().absorb(1).digest(),
    ///     Hasher::with_seed(2).absorb(1).digest()
    /// );
    /// ```
    #[must_use]
    pub const fn with_seed(seed: u64) -> Self {
        Self {
            state: seed,
            len: 0,
        }
    }

    /// Absorbs one word, consuming and returning `self`, which is the spelling
    /// a `const` item wants.
    ///
    /// ```
    /// # use corvid_hash::{Digest, Hasher};
    /// const MARK: Digest = Hasher::new().absorb(1).absorb(2).digest();
    /// assert_ne!(MARK, Hasher::new().absorb(2).absorb(1).digest());
    /// ```
    #[must_use]
    pub const fn absorb(self, word: u64) -> Self {
        Self {
            state: mix(self.state ^ word),
            len: self.len.wrapping_add(8),
        }
    }

    /// Injects the count of absorbed bytes, so a run of zero words cannot be
    /// trimmed without changing the answer, then mixes once more.
    ///
    /// Takes `&self`, so a running hash can be marked every tick without being
    /// rebuilt.
    #[must_use]
    pub const fn digest(&self) -> Digest {
        Digest(mix(self.state ^ self.len))
    }
}

impl Default for Hasher {
    fn default() -> Self {
        Self::new()
    }
}

/// Every method is overridden, for the reason [`Hasher`] documents: the defaults
/// forward in native endian and at the target's pointer width.
impl core::hash::Hasher for Hasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.digest().to_u64()
    }

    /// Absorbs bytes little-endian, eight at a time.
    ///
    /// A tail shorter than eight bytes is zero-extended, which alone would make
    /// `[1]` and `[1, 0]` collide. The true byte count separates them, because
    /// it is what [`digest`](Hasher::digest) injects.
    ///
    /// ```
    /// # use corvid_hash::digest;
    /// assert_ne!(digest(&[1_u8][..]), digest(&[1_u8, 0][..]));
    /// ```
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.state = mix(self.state ^ u64::from_le_bytes(word));
        }
        self.len = self.len.wrapping_add(bytes.len() as u64);
    }

    #[inline]
    fn write_u8(&mut self, value: u8) {
        self.write(&value.to_le_bytes());
    }

    #[inline]
    fn write_u16(&mut self, value: u16) {
        self.write(&value.to_le_bytes());
    }

    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.write(&value.to_le_bytes());
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        *self = self.absorb(value);
    }

    #[inline]
    fn write_u128(&mut self, value: u128) {
        self.write(&value.to_le_bytes());
    }

    /// As 64 bits, whatever the target's pointer width is, which is what keeps a
    /// `Vec`'s length prefix from differing between `wasm32` and `x86_64`.
    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }

    #[inline]
    fn write_i8(&mut self, value: i8) {
        self.write(&value.to_le_bytes());
    }

    #[inline]
    fn write_i16(&mut self, value: i16) {
        self.write(&value.to_le_bytes());
    }

    #[inline]
    fn write_i32(&mut self, value: i32) {
        self.write(&value.to_le_bytes());
    }

    #[inline]
    fn write_i64(&mut self, value: i64) {
        self.write(&value.to_le_bytes());
    }

    #[inline]
    fn write_i128(&mut self, value: i128) {
        self.write(&value.to_le_bytes());
    }

    /// Sign-extended to 64 bits, matching `write_usize`. A derived [`Hash`] for
    /// an enum absorbs its discriminant through here.
    #[inline]
    fn write_isize(&mut self, value: isize) {
        self.write_i64(value as i64);
    }
}

/// The digest of anything [`Hash`] can absorb.
///
/// ```
/// # use corvid_hash::digest;
/// #[derive(Hash)]
/// enum Contact { Lost, Tracking(i16) }
///
/// assert_eq!(digest(&Contact::Lost), digest(&Contact::Lost));
/// assert_ne!(digest(&Contact::Lost), digest(&Contact::Tracking(0)));
/// ```
#[must_use]
pub fn digest<T: Hash + ?Sized>(value: &T) -> Digest {
    let mut hasher = Hasher::new();
    value.hash(&mut hasher);
    hasher.digest()
}

/// The digest of `value` under a seed of your own.
///
/// ```
/// # use corvid_hash::{digest, digest_with_seed};
/// assert_ne!(digest(&7_u64), digest_with_seed(&7_u64, 1));
/// ```
#[must_use]
pub fn digest_with_seed<T: Hash + ?Sized>(value: &T, seed: u64) -> Digest {
    let mut hasher = Hasher::with_seed(seed);
    value.hash(&mut hasher);
    hasher.digest()
}
