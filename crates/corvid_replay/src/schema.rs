//! The digest an opening carries so that a capture from an incompatible build
//! is refused rather than replayed.

use core::hash::Hasher as _;

use corvid_hash::{Digest, Hasher};

/// A description of a game's types, hashed the same way on every build.
///
/// [`Opening::schema`](crate::Opening::schema) is compared when a session is
/// loaded, and a mismatch is [`Load::Schema`](crate::Load::Schema). This is how
/// that digest is produced.
///
/// # What this is, exactly
///
/// It hashes strings a person wrote. Rust has no reflection this crate could
/// use, so nothing here reads a game's types, compares them against the
/// description, or notices when the two stop matching. A description that is
/// not updated when a field is added produces the same digest under both
/// builds, and the capture then loads and diverges — which is precisely the
/// failure the schema exists to prevent, still uncaught, one level up.
///
/// So this buys one thing and it is worth stating narrowly: two builds that
/// describe themselves differently are told apart, cheaply, at load rather than
/// at the first tick where their states differ. Keeping the description in step
/// with the types is a person's job, and the smallest honest habit is to edit
/// the description in the same commit as the type — a `git` diff that touches
/// one and not the other is the thing to look for.
///
/// ```
/// use corvid_replay::Schema;
///
/// let before = Schema::new("counter")
///     .field("State.count", "i64")
///     .field("State.movers", "Vec<PlayerId>")
///     .digest();
///
/// // A field added to the state, described. The digest moves, so a capture
/// // recorded under the old build refuses to load under the new one.
/// let after = Schema::new("counter")
///     .field("State.count", "i64")
///     .field("State.movers", "Vec<PlayerId>")
///     .field("State.roster", "Vec<ProfileId>")
///     .digest();
/// assert_ne!(before, after);
///
/// // The order of the parts is part of the description, so two builds that
/// // list the same fields in a different order are told apart too.
/// let reordered = Schema::new("counter")
///     .field("State.movers", "Vec<PlayerId>")
///     .field("State.count", "i64")
///     .digest();
/// assert_ne!(before, reordered);
///
/// // And the limit, which no assertion can show because it is an absence:
/// // widening `count` to `i128` in the game's source and leaving `"i64"`
/// // written here produces `before` again, under both builds, and the capture
/// // loads.
/// ```
#[derive(Clone, Debug)]
pub struct Schema {
    /// The chain so far.
    hasher: Hasher,
}

impl Schema {
    /// Starts a description, named for the game it describes.
    ///
    /// The name is absorbed like any other part, so two games that happen to
    /// describe the same fields still describe different schemas.
    #[must_use]
    pub fn new(game: &str) -> Self {
        let mut hasher = Hasher::new();
        hasher.write(game.as_bytes());
        Self { hasher }
    }

    /// Adds one named part, and what it is.
    ///
    /// Both halves are absorbed with their lengths in front of them, and the
    /// length is what separates a part from an empty one beside it. The hasher
    /// pads every write to a whole word, so two parts of different content are
    /// already told apart by their bytes; what that does *not* tell apart is
    /// which side of the pair an empty string was on, because an empty write
    /// absorbs no word at all.
    ///
    /// ```
    /// use corvid_replay::Schema;
    ///
    /// // A part with no type against a type with no part. Without the lengths
    /// // these are one word in one order and the same word in the same order,
    /// // and two builds describing different things would agree.
    /// assert_ne!(
    ///     Schema::new("g").field("a", "").digest(),
    ///     Schema::new("g").field("", "a").digest(),
    /// );
    /// ```
    #[must_use]
    pub fn field(mut self, name: &str, of: &str) -> Self {
        self.hasher
            .write_u64(u64::try_from(name.len()).unwrap_or(u64::MAX));
        self.hasher.write(name.as_bytes());
        self.hasher
            .write_u64(u64::try_from(of.len()).unwrap_or(u64::MAX));
        self.hasher.write(of.as_bytes());
        self
    }

    /// The digest of everything described so far.
    #[must_use]
    pub const fn digest(&self) -> Digest {
        self.hasher.digest()
    }
}
