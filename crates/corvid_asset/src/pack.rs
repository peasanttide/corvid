//! A manifest bolted to the files it describes, and the two lines of it that
//! reach a digest.

use alloc::{boxed::Box, vec::Vec};
use core::{fmt, hash::Hash};

use corvid_files::{Missing, Source};
use corvid_hash::{Digest, Hasher};

use crate::{Manifest, PackId};

/// The half of a pack that reaches [`Stack::digest`](crate::Stack::digest): who
/// it is and which revision.
///
/// Not the files. Two peers compare this before either has read a byte, and the
/// question it settles is whether they are running the same content set at all
/// -- which is a question about names and versions, answerable at seating from
/// a manifest each side already has. Whether the bytes behind those names match
/// is [`Pack::content`]'s question, it costs a read of every file in the pack,
/// and it is a thing a build does rather than a thing a lobby does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackStamp {
    /// Which pack.
    pub id: PackId,
    /// Which revision of it.
    pub version: u32,
}

/// A manifest and the files it describes.
///
/// The source is not handed out mutably and cannot be: [`source`](Self::source)
/// answers a `&dyn Source`, and `Source::write` takes `&mut self` and is not
/// forwarded by the blanket impl on `&T`. So "a level cannot edit the pack it
/// was mounted over" is a thing that fails to compile rather than a rule
/// somebody has to remember, which is the same guarantee `corvid_files` builds
/// for a level handed a `&dyn Source` and for the same reason.
pub struct Pack {
    manifest: Manifest,
    source: Box<dyn Source>,
}

impl Pack {
    /// A pack read out of `source`.
    ///
    /// Generic over the source and boxed here rather than taking a
    /// `Box<dyn Source>`, so that a caller with a directory reader, an archive
    /// or a [`Memory`](corvid_files::Memory) writes the same line. A stack
    /// mixes them freely once they are in, which is why the box is not a
    /// parameter of [`Stack`](crate::Stack).
    #[must_use]
    pub fn new<S: Source + 'static>(manifest: Manifest, source: S) -> Self {
        Self {
            manifest,
            source: Box::new(source),
        }
    }

    /// What the pack says about itself.
    #[must_use]
    pub const fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The files, readable and not writable.
    #[must_use]
    pub fn source(&self) -> &dyn Source {
        &*self.source
    }

    /// The two lines of the manifest a digest is taken over.
    #[must_use]
    pub fn stamp(&self) -> PackStamp {
        PackStamp {
            id: self.manifest.id,
            version: self.manifest.version,
        }
    }

    /// A digest of every byte in the pack, in sorted path order.
    ///
    /// The expensive counterpart to [`stamp`](Self::stamp), and a different
    /// question: a stamp says which pack this claims to be and this says what is
    /// actually in it, so an edited file under an unchanged version shows up
    /// here and nowhere else. It reads the whole pack, which is why it is a
    /// separate call and why it belongs in a build step or a validator rather
    /// than on the path a lobby takes.
    ///
    /// The order is [`Source::list`]'s, which that trait requires to be sorted,
    /// and each file contributes its path as well as its bytes -- so a pack that
    /// renamed a file without changing its contents digests differently, which
    /// it must, since a path is what an override is addressed by.
    ///
    /// # Errors
    ///
    /// [`Missing`] if the source cannot be listed, or if a path it listed
    /// cannot then be read.
    pub fn content(&self) -> Result<Digest, Missing> {
        let paths = self.source.list()?;
        let mut hasher = Hasher::new();
        // The count first, so that a pack of one empty file and a pack of none
        // cannot reach the same state by absorbing nothing.
        paths.len().hash(&mut hasher);
        for path in &paths {
            let bytes: Vec<u8> = self.source.read(path)?;
            path.hash(&mut hasher);
            bytes.hash(&mut hasher);
        }
        Ok(hasher.digest())
    }
}

/// The manifest, and a count of the files rather than the source itself.
///
/// Written rather than derived because there is no derive available: a
/// `Box<dyn Source>` prints only what its trait allows, and `Source` does not
/// require `Debug` -- deliberately, since the interesting implementations are a
/// directory and an archive and neither has a useful short form. The count is
/// what a reader wants from a source anyway, and it is a read of the listing,
/// so a source that cannot be asked prints as unknown rather than failing
/// inside a formatter.
impl fmt::Debug for Pack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pack")
            .field("manifest", &self.manifest)
            .field("files", &self.source.list().map(|paths| paths.len()).ok())
            .finish_non_exhaustive()
    }
}
