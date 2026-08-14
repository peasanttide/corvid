//! The hand-written encodings for [`Opening`](crate::Opening) and
//! [`Session`](crate::Session).
//!
//! Hand-written rather than derived because both carry a type parameter whose
//! bounds a derive would over-constrain: a derived `Clone` on `Opening<S>`
//! would demand `S: Clone` rather than `S: State`, and a derived `Debug` would
//! demand it of every associated type. Every impl here is that same correction,
//! which is why they are together and away from the structures they belong
//! to.

use alloc::{string::String, sync::Arc, vec::Vec};
use core::fmt;

use corvid_behavior::State;
use corvid_hash::Digest;
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

use crate::opening::{Opening, Profile, Seed};
use crate::session::Session;
use crate::{ActionLog, HashTrace};

// Every derive below would put a bound on `G` -- `G: Clone`, `G: Serialize` --
// and `G` is a marker type with no fields that satisfies none of them. What
// these types are made of is `G`'s associated types, all four of which are
// `Data`, so the bounds the derives want are already in the `State` bound
// and the ones they would add are wrong. Hence `#[serde(bound = "")]` and the
// hand-written rest.

impl<S: State> Serialize for Opening<S> {
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        #[derive(Serialize)]
        #[serde(bound = "")]
        struct Wire<'a, S: State> {
            level: &'a str,
            /// The value and not the handle, for this field and for `rules` and
            /// `origin` below.
            ///
            /// The three of them are [`Arc`]s in the struct, and this shim is
            /// what pins the encoding to what those three values are rather than
            /// to how they happen to be held. `serde`'s `rc` feature is on in
            /// this build -- `Session::seek` hands back an `Arc<S>` and
            /// something has to be able to write one down -- so the naive
            /// derive would now compile and would produce the same bytes today,
            /// since `serde`'s `Arc` implementations read straight through to
            /// what they point at. What it would give up is the guarantee: the
            /// format of a capture would then be a property of a feature flag
            /// and of an upstream crate's choices about handles, and this is the
            /// one type in the workspace where that has to be a property of the
            /// source instead. The `&'a` fields deref-coerce out of the `Arc`s
            /// with no cast anywhere, so keeping them costs nothing and saying
            /// so here is the whole of the maintenance burden.
            content: &'a S::Level,
            rules: &'a S::Rules,
            roster: &'a [Profile],
            seed: &'a Seed,
            first: &'a Tick,
            origin: &'a S,
            schema: u64,
        }

        // Resolved rather than optional on the wire: a written-down session
        // always opens on a definite state, so the file format is unchanged by
        // the field having become an `Option` in memory.
        let origin = self.origin();

        Wire::<S> {
            level: &self.level,
            content: &self.content,
            rules: &self.rules,
            roster: &self.roster,
            seed: &self.seed,
            first: &self.first,
            origin: &origin,
            schema: self.schema.to_u64(),
        }
        .serialize(serializer)
    }
}

impl<'de, S: State> Deserialize<'de> for Opening<S> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(bound = "")]
        struct Wire<S: State> {
            level: String,
            content: S::Level,
            rules: S::Rules,
            roster: Vec<Profile>,
            seed: Seed,
            first: Tick,
            origin: S,
            schema: u64,
        }

        let wire = Wire::<S>::deserialize(deserializer)?;
        Ok(Self {
            level: wire.level,
            content: Arc::new(wire.content),
            rules: Arc::new(wire.rules),
            roster: wire.roster,
            seed: wire.seed,
            first: wire.first,
            origin: Some(Arc::new(wire.origin)),
            schema: Digest::from_u64(wire.schema),
        })
    }
}

impl<S: State> Clone for Opening<S> {
    fn clone(&self) -> Self {
        Self {
            level: self.level.clone(),
            content: Arc::clone(&self.content),
            rules: Arc::clone(&self.rules),
            roster: self.roster.clone(),
            seed: self.seed,
            first: self.first,
            origin: self.origin.clone(),
            schema: self.schema,
        }
    }
}

impl<S: State> fmt::Debug for Opening<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Opening")
            .field("level", &self.level)
            .field("content", &self.content)
            .field("rules", &self.rules)
            .field("roster", &self.roster)
            .field("seed", &self.seed)
            .field("first", &self.first)
            .field("origin", &self.origin)
            .field("schema", &self.schema)
            .finish()
    }
}

/// Equal values, never equal handles.
///
/// The three [`Arc`] fields are dereferenced rather than compared as handles, so
/// an opening that has been through a capture -- which rebuilds every one of them
/// as a fresh allocation -- is equal to the one that was written down. That is
/// what `tests/roundtrip.rs` asserts, and it would be an assertion about
/// addresses if this compared what it holds rather than what it points at.
impl<S: State> PartialEq for Opening<S> {
    fn eq(&self, other: &Self) -> bool {
        self.level == other.level
            && *self.content == *other.content
            && *self.rules == *other.rules
            && self.roster == other.roster
            && self.seed == other.seed
            && self.first == other.first
            // Resolved on both sides, so an opening that carried no origin and
            // one that carried `S::default()` explicitly compare equal -- which
            // is right, because they open the same session.
            && *self.origin() == *other.origin()
            && self.schema == other.schema
    }
}

impl<S: State> Eq for Opening<S> {}

impl<S: State> Serialize for Session<S> {
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        #[derive(Serialize)]
        #[serde(bound = "")]
        struct Wire<'a, S: State> {
            opening: &'a Opening<S>,
            log: &'a ActionLog<S::Action>,
            marks: &'a HashTrace,
        }

        Wire::<S> {
            opening: &self.opening,
            log: &self.log,
            marks: &self.marks,
        }
        .serialize(serializer)
    }
}

impl<'de, S: State> Deserialize<'de> for Session<S> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(bound = "")]
        struct Wire<S: State> {
            opening: Opening<S>,
            log: ActionLog<S::Action>,
            marks: HashTrace,
        }

        let wire = Wire::<S>::deserialize(deserializer)?;
        Ok(Self {
            opening: wire.opening,
            log: wire.log,
            marks: wire.marks,
        })
    }
}

impl<S: State> Clone for Session<S> {
    fn clone(&self) -> Self {
        Self {
            opening: self.opening.clone(),
            log: self.log.clone(),
            marks: self.marks.clone(),
        }
    }
}

impl<S: State> fmt::Debug for Session<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("opening", &self.opening)
            .field("log", &self.log)
            .field("marks", &self.marks)
            .finish()
    }
}

impl<S: State> PartialEq for Session<S> {
    fn eq(&self, other: &Self) -> bool {
        self.opening == other.opening && self.log == other.log && self.marks == other.marks
    }
}

impl<S: State> Eq for Session<S> {}
