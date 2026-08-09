//! The second obligation the type system asks for and cannot check: a value
//! has to come back the way it went in.

use alloc::string::{String, ToString};
use core::fmt;

use corvid_hash::{Digest, digest};

use crate::Data;

/// A value did not survive being written down and read back.
///
/// Every case here is a desync waiting for the runtime to send a snapshot, and
/// none of them is visible from inside a single peer's simulation: the state a
/// game computes is right, and the state its neighbour reconstructs from the
/// bytes is not the same one.
#[derive(Clone, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum Unfaithful {
    /// The value could not be written down at all, with the format's reason.
    #[error("the value could not be serialized: {0}")]
    Wrote(String),
    /// The bytes could not be read back, with the format's reason.
    ///
    /// A type that needs the field names alongside the values -- `#[serde(flatten)]`,
    /// an untagged enum -- lands here, because a snapshot format does not carry
    /// them. That is a finding rather than a false alarm: it is the shape of a
    /// type that cannot be sent compactly, and the runtime sends states
    /// compactly.
    #[error("what was written down did not read back: {0}")]
    Read(String),
    /// It came back, and it is not what went in.
    ///
    /// The message depends on whether the two digests agree, which is why this
    /// variant formats through a function rather than a literal: two equal
    /// digests here mean the round trip lost something this game's `Eq` can see
    /// and its `Hash` cannot, and that is worth saying rather than printing one
    /// digest twice.
    #[error(fmt = changed)]
    Changed {
        /// The digest of the value that was written down.
        before: Digest,
        /// The digest of the value that came back.
        after: Digest,
    },
}

/// How [`Unfaithful::Changed`] reads, which depends on its two digests.
///
/// Equal digests and unequal values is the trap this check exists for: the
/// round trip did lose something, and the digest is the half that cannot see
/// it. A message
/// that printed one digest twice would look like the check contradicting itself,
/// so that case says what it means instead.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the signature is the derive's rather than this crate's: `#[error(fmt = ...)]` hands each field of the variant over by reference, and a `Digest` taken by value does not typecheck against it"
)]
fn changed(before: &Digest, after: &Digest, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if before == after {
        write!(
            f,
            "the value that came back compares unequal and digests alike \
             ({before}): the round trip lost something this game's Eq can \
             see and its Hash cannot"
        )
    } else {
        write!(
            f,
            "the value changed in the round trip: {before} went in and \
             {after} came back"
        )
    }
}

/// Serializes `value`, deserializes it, and reports whether what came back is
/// the same value -- by comparison and by digest.
///
/// [`Data`] demands `Serialize + DeserializeOwned` and cannot demand that the
/// two agree. A `#[serde(skip)]` on a field compiles, satisfies every other
/// check this crate offers, and desyncs the first time the runtime sends a
/// snapshot: the peer that computed the state has the field and the peer that
/// received it has `Default::default()`. A hand-written `Deserialize` that
/// forgets a field, a `#[serde(into = "...")]` whose conversion is lossy, and a
/// `#[serde(default)]` covering for a field the writer never emits are the same
/// bug wearing different clothes. This is the mechanical form of the obligation:
/// point it at a state, a level, a set of rules or an action, and it says
/// whether that value can be sent.
///
/// Both comparisons, because the runtime makes both: a rollback decides
/// whether its prediction held with `Eq`, a desync check decides whether two
/// peers agree with the digest, and a game's `Eq` and its `Hash` can disagree
/// about what a value is. A round trip either of
/// them can see through is a round trip that lost something.
///
/// # What a pass establishes, and what it does not
///
/// It checks *one* value. A type whose fields are all faithful except on the
/// variant this value does not happen to be is clean here and broken in play, so
/// point it at the states a session actually reaches -- the joining tick, the
/// tick a level is loaded on -- and not only at `State::default()`, which is the
/// value most likely to survive anything because it is what a lost field decays
/// to.
///
/// It also checks one *format*: `corvid_wire`, which is the one a snapshot is
/// written down in, and which carries no field names -- so a type that only
/// survives a self-describing encoding reports as unfaithful here rather than
/// passing and then failing on the wire. The failures that matter most -- a
/// skipped field, a lossy conversion -- are properties of the type's serde
/// implementation and would show up in any format; the ones that need this one
/// are the types that cannot be written down compactly at all.
///
/// ```
/// use corvid_behavior::round_trip_is_faithful;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
/// struct Faithful { count: i64 }
///
/// #[derive(Clone, Debug, Default, Hash, PartialEq, Eq, Serialize, Deserialize)]
/// struct Skipped {
///     count: i64,
///     // Never written down, and `Default::default()` on the way back -- so the
///     // peer that receives this state is playing a slightly different game.
///     #[serde(skip)]
///     seed: i64,
/// }
///
/// assert!(round_trip_is_faithful(&Faithful { count: 7 }).is_ok());
/// assert!(round_trip_is_faithful(&Skipped { count: 7, seed: 9 }).is_err());
///
/// // And the value that hides it: a skipped field whose value already is the
/// // default comes back unchanged, which is why this is checked against the
/// // states a session reaches rather than against a fresh one.
/// assert!(round_trip_is_faithful(&Skipped { count: 7, seed: 0 }).is_ok());
/// ```
///
/// # Errors
///
/// [`Unfaithful::Wrote`] if the value could not be serialized,
/// [`Unfaithful::Read`] if the bytes could not be deserialized, and
/// [`Unfaithful::Changed`] if the value that came back differs from the one that
/// went in by comparison, by digest, or by both.
pub fn round_trip_is_faithful<T: Data>(value: &T) -> Result<(), Unfaithful> {
    let bytes = corvid_wire::encode(value).map_err(|why| Unfaithful::Wrote(why.to_string()))?;
    let back: T = corvid_wire::decode(&bytes).map_err(|why| Unfaithful::Read(why.to_string()))?;

    let before = digest(value);
    let after = digest(&back);
    if before != after || &back != value {
        return Err(Unfaithful::Changed { before, after });
    }
    Ok(())
}
