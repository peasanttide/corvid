//! Live tunables, which propose and never write.

use core::fmt;
use core::hash::{Hash, Hasher};
use core::ops::RangeInclusive;

use crate::Invalid;
use corvid_fixed::I16F16;
use corvid_time::Tick;

/// A field of a game's `Rules` a console may change, named by a path and
/// reached by a pair of accessors the game supplies.
///
/// The accessors are private. A caller holding a `Tunable` can read a `Rules`
/// and can ask for a [`Proposal`]; there is no method here, and no field, that
/// writes into one.
pub struct Tunable<R> {
    /// What the console calls it.
    pub path: &'static str,
    /// What it may be set to, ends included.
    pub range: RangeInclusive<I16F16>,
    read: fn(&R) -> I16F16,
    write: fn(&mut R, I16F16),
}

impl<R> Tunable<R> {
    /// A tunable over one field.
    ///
    /// ```
    /// use corvid_dev::Tunable;
    /// use corvid_fixed::I16F16;
    ///
    /// #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    /// struct Rules {
    ///     damage: I16F16,
    /// }
    ///
    /// const DAMAGE: Tunable<Rules> = Tunable::new(
    ///     "tower.arc.damage",
    ///     I16F16::ZERO..=I16F16::from_f64(500.0),
    ///     |rules| rules.damage,
    ///     |rules, to| rules.damage = to,
    /// );
    ///
    /// assert_eq!(DAMAGE.read(&Rules { damage: I16F16::ONE }), I16F16::ONE);
    /// ```
    #[must_use]
    pub const fn new(
        path: &'static str,
        range: RangeInclusive<I16F16>,
        read: fn(&R) -> I16F16,
        write: fn(&mut R, I16F16),
    ) -> Self {
        Self {
            path,
            range,
            read,
            write,
        }
    }

    /// What this tunable currently reads as.
    #[must_use]
    #[inline]
    pub fn read(&self, rules: &R) -> I16F16 {
        (self.read)(rules)
    }

    /// Whether the range allows `to`.
    #[must_use]
    #[inline]
    pub fn allows(&self, to: I16F16) -> bool {
        self.range.contains(&to)
    }

    /// The lowest value it takes.
    #[must_use]
    #[inline]
    pub const fn low(&self) -> I16F16 {
        *self.range.start()
    }

    /// The highest.
    #[must_use]
    #[inline]
    pub const fn high(&self) -> I16F16 {
        *self.range.end()
    }
}

/// The path, the range and the two accessors, and no `R`: a tunable holds
/// function pointers rather than a `Rules`, so it compares and hashes whatever
/// the game's rules are.
impl<R> Clone for Tunable<R> {
    fn clone(&self) -> Self {
        Self {
            path: self.path,
            range: self.range.clone(),
            read: self.read,
            write: self.write,
        }
    }
}

/// The path and the range, which is everything about a tunable that can be
/// printed: the other two fields are function pointers.
impl<R> fmt::Debug for Tunable<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tunable")
            .field("path", &self.path)
            .field("range", &self.range)
            .finish_non_exhaustive()
    }
}

/// Two tunables are the same tunable when they name the same path, the same
/// range and the same pair of functions.
///
/// Hand-written because a derive would put `R: PartialEq` on the impl for a
/// parameter that appears only inside the function pointers, and because
/// comparing those needs [`fn_addr_eq`](core::ptr::fn_addr_eq).
impl<R> PartialEq for Tunable<R> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.range == other.range
            && core::ptr::fn_addr_eq(self.read, other.read)
            && core::ptr::fn_addr_eq(self.write, other.write)
    }
}

impl<R> Eq for Tunable<R> {}

/// The path and the range, which is the half of [`PartialEq`] a hash can
/// see: a function pointer's address is not something to hash a registry key
/// by, since the same function has a different one in another build.
impl<R> Hash for Tunable<R> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.range.hash(state);
    }
}

/// What changing a tunable produces. **Never a mutation.**
///
/// `Rules` is hashed, so a peer that changed one locally would desync on the
/// next tick. A proposal is a whole `Rules` value every peer must accept: a
/// single-player session applies it immediately and a multiplayer session sends
/// it, and neither path can be written wrongly because there is no other path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Proposal<R> {
    /// The whole `Rules`, with the one field changed.
    pub rules: R,
    /// The tunable this came from, which is what a peer's prompt shows.
    pub because: &'static str,
    /// The tick it would take effect on.
    pub at: Tick,
}

impl<R> Proposal<R> {
    /// The rules it proposes.
    #[must_use]
    #[inline]
    pub fn into_rules(self) -> R {
        self.rules
    }
}

/// Every tunable a game registered, sorted by path.
pub struct Tuning<R> {
    tunables: Vec<Tunable<R>>,
}

impl<R> Tuning<R> {
    /// Nothing registered.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            tunables: Vec::new(),
        }
    }

    /// Register one. Registering a path twice replaces what was there.
    pub fn register(&mut self, tunable: Tunable<R>) {
        match self
            .tunables
            .binary_search_by(|held| held.path.cmp(tunable.path))
        {
            Ok(at) => {
                if let Some(slot) = self.tunables.get_mut(at) {
                    *slot = tunable;
                }
            }
            Err(at) => self.tunables.insert(at, tunable),
        }
    }

    /// The tunable under `path`.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&Tunable<R>> {
        self.tunables
            .binary_search_by(|held| held.path.cmp(path))
            .ok()
            .and_then(|at| self.tunables.get(at))
    }

    /// Every path, in order.
    pub fn paths(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.tunables.iter().map(|tunable| tunable.path)
    }

    /// What one tunable currently reads as.
    #[must_use]
    pub fn read(&self, rules: &R, path: &str) -> Option<I16F16> {
        self.get(path).map(|tunable| tunable.read(rules))
    }

    /// How many are registered.
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.tunables.len()
    }

    /// Whether none are.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.tunables.is_empty()
    }
}

impl<R: Clone> Tuning<R> {
    /// Build a proposal. The live `Rules` is untouched.
    ///
    /// ```
    /// use corvid_dev::{Tunable, Tuning};
    /// use corvid_hash::digest;
    /// use corvid_time::Tick;
    /// use corvid_fixed::I16F16;
    ///
    /// #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    /// struct Rules {
    ///     damage: I16F16,
    /// }
    ///
    /// let mut tuning = Tuning::new();
    /// tuning.register(Tunable::new(
    ///     "tower.arc.damage",
    ///     I16F16::ZERO..=I16F16::from_f64(500.0),
    ///     |rules: &Rules| rules.damage,
    ///     |rules: &mut Rules, to| rules.damage = to,
    /// ));
    ///
    /// let live = Rules { damage: I16F16::ONE };
    /// let before = digest(&live);
    ///
    /// let proposal = tuning.propose(&live, "tower.arc.damage", I16F16::from_f64(12.5), Tick(40))?;
    ///
    /// // The rules that were handed in are exactly what they were.
    /// assert_eq!(digest(&live), before);
    /// // And the proposal reaches the hash, which is why every peer must take it.
    /// assert_ne!(digest(&proposal.rules), before);
    /// assert_eq!(proposal.at, Tick(40));
    /// # Ok::<(), corvid_dev::Invalid>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`Invalid::Unknown`] for a path nothing is registered under, and
    /// [`Invalid::OutOfRange`] naming the range for a value outside it.
    pub fn propose(
        &self,
        rules: &R,
        path: &str,
        to: I16F16,
        at: Tick,
    ) -> Result<Proposal<R>, Invalid> {
        let Some(tunable) = self.get(path) else {
            return Err(Invalid::Unknown {
                path: path.to_owned(),
            });
        };
        if !tunable.allows(to) {
            return Err(Invalid::OutOfRange {
                path: tunable.path,
                low: tunable.low(),
                high: tunable.high(),
                given: to,
            });
        }
        let mut rules = rules.clone();
        (tunable.write)(&mut rules, to);
        Ok(Proposal {
            rules,
            because: tunable.path,
            at,
        })
    }
}

/// An empty registry.
///
/// Hand-written because a derive would put `R: Default` on the impl, and the
/// one field is a `Vec` that has a default whatever `R` is.
impl<R> Default for Tuning<R> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// The paths, as a list. What a reader wants of a registry is what is in it.
impl<R> fmt::Debug for Tuning<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.paths()).finish()
    }
}

impl<R> FromIterator<Tunable<R>> for Tuning<R> {
    fn from_iter<I: IntoIterator<Item = Tunable<R>>>(tunables: I) -> Self {
        let mut tuning = Self::new();
        for tunable in tunables {
            tuning.register(tunable);
        }
        tuning
    }
}
