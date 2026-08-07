//! Bounded names, stored inline.
//!
//! A name that identifies something across a network and across a save file
//! has to be stable, comparable, and cheap enough to put inside an enum that a
//! tick returns by value. A `String` is none of the last three: it allocates,
//! it makes its enum sixteen bytes wider than the payload it carries, and it
//! drags a lifetime or a clone through every place a name is copied.
//!
//! So a name is a fixed array of bytes with the unused tail left zero. NUL is
//! not a character any name may contain, which is what makes the padding
//! unambiguous — and it is also what makes the derived `Ord` correct, since
//! zero sorts below every byte a name is allowed to hold and so the array
//! order and the string order are the same order.
//!
//! `Eq`, `Ord` and `Hash` are all derived over the array, so they agree with
//! each other and the capacity is part of the digest: a name type's width is a
//! wire-format decision rather than an implementation one.

use core::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// Why a string could not become a name.
///
/// Both cases are refusals rather than repairs. Truncating a name that does not
/// fit would let two different levels answer to one identifier, and a save
/// written against the second would load the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum InvalidName {
    /// The string is longer than the name type asked to hold it.
    TooLong {
        /// How many bytes the string has.
        len: usize,
        /// How many bytes the name type holds.
        capacity: usize,
    },
    /// The string contains a NUL byte, which the padding needs to itself.
    InteriorNul {
        /// The byte offset of the first NUL.
        at: usize,
    },
}

impl fmt::Display for InvalidName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::TooLong { len, capacity } => {
                write!(f, "name is {len} bytes; the limit is {capacity}")
            }
            Self::InteriorNul { at } => write!(f, "name contains a NUL byte at offset {at}"),
        }
    }
}

impl core::error::Error for InvalidName {}

/// `N` bytes of name, NUL-padded.
///
/// Private, because the capacity is part of each public name type's meaning
/// rather than something a caller should be choosing at the use site.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Name<const N: usize>([u8; N]);

impl<const N: usize> Name<N> {
    /// The empty name, which is what [`Default`] gives.
    pub(crate) const EMPTY: Self = Self([0; N]);

    /// Copies `name` in, or says why it does not fit.
    pub(crate) const fn new(name: &str) -> Result<Self, InvalidName> {
        let bytes = name.as_bytes();
        if bytes.len() > N {
            return Err(InvalidName::TooLong {
                len: bytes.len(),
                capacity: N,
            });
        }
        let mut stored = [0; N];
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] == 0 {
                return Err(InvalidName::InteriorNul { at });
            }
            stored[at] = bytes[at];
            at += 1;
        }
        Ok(Self(stored))
    }

    /// How many bytes of the array the name occupies.
    pub(crate) const fn len(&self) -> usize {
        let mut at = 0;
        while at < N && self.0[at] != 0 {
            at += 1;
        }
        at
    }

    /// The name, without its padding.
    pub(crate) const fn as_str(&self) -> &str {
        let (name, _padding) = self.0.as_slice().split_at(self.len());
        match core::str::from_utf8(name) {
            Ok(text) => text,
            // Unreachable: the only two ways to build a `Name` are `new`, which
            // copies the bytes of a `&str`, and `Deserialize`, which goes
            // through `new`. Returning the empty name rather than asserting
            // keeps a panic out of a `const fn` that is on the path of every
            // comparison and every log line.
            Err(_) => "",
        }
    }
}

impl<const N: usize> Default for Name<N> {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl<const N: usize> fmt::Display for Name<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A name serializes as the string it is, not as the array it is stored in.
///
/// A captured session file is read by people, and a level named
/// `"terminus"` should say so rather than list fifteen numbers. It also means
/// the capacity can grow later without invalidating anything already written.
impl<const N: usize> Serialize for Name<N> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Deserializing re-checks the bounds, because a file is not a `&str` this
/// program built and a name that did not fit would otherwise be silently cut.
impl<'de, const N: usize> Deserialize<'de> for Name<N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(NameVisitor::<N>)
    }
}

/// Reads a name from a string, rejecting anything [`Name::new`] would reject.
struct NameVisitor<const N: usize>;

impl<const N: usize> de::Visitor<'_> for NameVisitor<N> {
    type Value = Name<N>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a name of at most {N} bytes with no NUL in it")
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<Self::Value, E> {
        Name::new(text).map_err(E::custom)
    }
}

/// Declares a public name type over a private [`Name`] of the given capacity.
///
/// Every one of these types is the same code with a different bound and a
/// different meaning, and writing them out would be five identical bugs waiting
/// for one of the five copies to be fixed.
macro_rules! bounded_name {
    (
        $(#[$meta:meta])*
        $name:ident, $capacity:literal
    ) => {
        $(#[$meta])*
        #[derive(
            Clone,
            Copy,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name($crate::name::Name<$capacity>);

        impl $name {
            /// The most bytes a name of this kind may have.
            pub const CAPACITY: usize = $capacity;

            /// The empty name.
            pub const EMPTY: Self = Self($crate::name::Name::EMPTY);

            /// Builds one, or says why the string does not fit.
            ///
            /// # Errors
            ///
            /// [`InvalidName::TooLong`](crate::InvalidName::TooLong) if the
            /// string is longer than [`CAPACITY`](Self::CAPACITY), and
            /// [`InvalidName::InteriorNul`](crate::InvalidName::InteriorNul)
            /// if it contains a NUL byte.
            pub const fn new(name: &str) -> ::core::result::Result<Self, $crate::InvalidName> {
                match $crate::name::Name::new(name) {
                    Ok(name) => Ok(Self(name)),
                    Err(why) => Err(why),
                }
            }

            /// The name itself.
            #[must_use]
            pub const fn as_str(&self) -> &str {
                self.0.as_str()
            }

            /// How many bytes long the name is.
            #[must_use]
            pub const fn len(&self) -> usize {
                self.0.len()
            }

            /// Whether the name is the empty one.
            #[must_use]
            pub const fn is_empty(&self) -> bool {
                self.0.len() == 0
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.0, f)
            }
        }

        impl ::core::fmt::Debug for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                write!(f, concat!(stringify!($name), "({:?})"), self.as_str())
            }
        }

    };
}

pub(crate) use bounded_name;
