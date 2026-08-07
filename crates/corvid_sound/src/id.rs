//! The numbered identifiers, which are all the same type with different names.
//!
//! Each is a newtype over an integer with the field public, because an
//! identifier is a number and hiding it behind an accessor would cost every
//! caller a line and buy nothing. What the newtype buys is that a
//! [`SoundId`](crate::SoundId) cannot be passed where a
//! [`BusId`](crate::BusId) was meant, which is a mistake that would otherwise
//! compile and then route a footstep through the recording of a footstep.
//!
//! What the numbers *mean* is not decided here. A [`SoundId`](crate::SoundId)
//! names a recording in whatever catalogue the game and its backend agree on,
//! and this crate never resolves one.

/// Declares one of them, with its serde and digest encodings.
///
/// The digest absorbs the integer and no type tag, which is the convention the
/// rest of the workspace hashes under: what establishes that two peers are
/// reading the same field is the opening's schema, not a tag on every value.
/// Two identifiers of different kinds holding the same number therefore digest
/// alike, and that is fine, because nothing ever hashes one out of context.
macro_rules! id_type {
    (
        $(#[$meta:meta])*
        $name:ident, $repr:ty, $field_doc:literal
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[cfg_attr(
            feature = "serde",
            derive(::serde::Serialize, ::serde::Deserialize),
            serde(transparent)
        )]
        pub struct $name(
            #[doc = $field_doc]
            pub $repr,
        );

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

id_type! {
    /// Which recording to play.
    ///
    /// Nothing here loads one, decodes one, or knows how long one is. The
    /// number is agreed between the game that emits it and the backend that
    /// turns the frame into samples, and a frame carrying an identifier no
    /// catalogue answers to is something the backend has to decide about — this
    /// crate stores it either way.
    SoundId, u32, "The catalogue number."
}

id_type! {
    /// Which mixing bus a source or a cue is routed through.
    BusId, u16, "The bus number."
}

id_type! {
    /// A continuously playing sound's identity from one frame to the next.
    ///
    /// A [`Source`](crate::Source) is a sound that is already playing, so a
    /// backend keeping a voice open for it has to know which voice a given
    /// entry in this frame belongs to. Matching on position would restart it
    /// every time the thing moved; matching on [`SoundId`] would merge two
    /// torches burning in the same room into one. So the extractor supplies an
    /// identity, and the obligation that it is stable while the sound plays,
    /// and unique within a frame, is the extractor's — nothing here checks
    /// either.
    SourceId, u32, "The source number."
}

impl BusId {
    /// The bus everything ends up on, and the one a [`Bus`](crate::Bus) may not
    /// usefully name as its own parent.
    ///
    /// Zero rather than a named constant elsewhere because a
    /// [`Default`]-constructed routing has to land somewhere audible, and the
    /// master bus is the only choice that does not depend on what a particular
    /// game called its buses.
    pub const MASTER: Self = Self(0);
}
