//! Reading a `.sf2` image into a [`Bank`].
//!
//! A `SoundFont` file is RIFF: a tree of four-byte-tagged chunks, three of which
//! matter. `INFO` holds the bank's name, `sdta` holds one pool of sixteen-bit
//! PCM, and `pdta` holds the nine parallel arrays the specification calls the
//! hydra -- presets, instruments and samples, each a header array and a set of
//! index arrays that bound one another. Every one of those bounds is checked
//! here, because a malformed bank is a file somebody downloaded and not a bug in
//! this crate.

use alloc::vec::Vec;

use crate::synth::bank::Bank;
use crate::synth::hydra::Hydra;

/// What went wrong reading a bank.
#[derive(Clone, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum BankError {
    /// The image is shorter than the header it claims to have.
    #[error("the image is {found} bytes, too short for a RIFF header")]
    Truncated {
        /// How many bytes there were.
        found: usize,
    },
    /// The image does not start with a RIFF chunk.
    #[error("the image does not begin with a RIFF chunk")]
    NotRiff,
    /// The RIFF form is something other than a `SoundFont`.
    #[error("the RIFF form is not 'sfbk'")]
    NotSoundFont,
    /// A chunk the format requires is absent.
    #[error("the '{chunk}' chunk is missing")]
    Missing {
        /// Which chunk.
        chunk: &'static str,
    },
    /// A hydra chunk's length is not a whole number of its records.
    #[error("the '{chunk}' chunk is {found} bytes, not a multiple of {size}")]
    RecordSize {
        /// Which chunk.
        chunk: &'static str,
        /// How long it was.
        found: usize,
        /// How long one record is.
        size: usize,
    },
    /// A record points past the end of the array it indexes.
    #[error("a '{chunk}' record indexes past the end of '{into}'")]
    OutOfRange {
        /// The chunk holding the index.
        chunk: &'static str,
        /// The chunk it indexes into.
        into: &'static str,
    },
}

/// One chunk: its four-byte tag and its payload.
type Chunk<'a> = ([u8; 4], &'a [u8]);

/// Splits `body` into the chunks it holds, stopping at the first malformed one.
///
/// A chunk is a tag, a little-endian length and that many bytes, padded to an
/// even boundary. Stopping rather than failing is deliberate: banks in the wild
/// carry trailing rubbish, and a bank whose three required chunks are all
/// present and well formed is readable whatever follows them.
fn chunks(body: &[u8]) -> Vec<Chunk<'_>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 8 <= body.len() {
        let Some(tag) = body
            .get(at..at + 4)
            .and_then(|s| <[u8; 4]>::try_from(s).ok())
        else {
            break;
        };
        let Some(length) = body
            .get(at + 4..at + 8)
            .and_then(|s| <[u8; 4]>::try_from(s).ok())
            .map(u32::from_le_bytes)
            .and_then(|value| usize::try_from(value).ok())
        else {
            break;
        };
        let Some(data) = body.get(at + 8..at + 8 + length) else {
            break;
        };
        out.push((tag, data));
        at += 8 + length + (length % 2);
    }
    out
}

/// The payload of the chunk tagged `tag`, if it is there.
fn find<'a>(list: &[Chunk<'a>], tag: [u8; 4]) -> Option<&'a [u8]> {
    list.iter()
        .find(|(held, _)| *held == tag)
        .map(|(_, data)| *data)
}

/// The payload of the `LIST` whose type is `kind`, if it is there.
fn list<'a>(chunks: &[Chunk<'a>], kind: [u8; 4]) -> Option<&'a [u8]> {
    chunks.iter().find_map(|(tag, data)| {
        if tag != b"LIST" {
            return None;
        }
        let (head, rest) = data.split_at_checked(4)?;
        if head == kind { Some(rest) } else { None }
    })
}

/// A NUL-terminated, space-padded fixed-width name.
pub(crate) fn name(bytes: &[u8]) -> alloc::string::String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let text = bytes.get(..end).unwrap_or(bytes);
    alloc::string::String::from_utf8_lossy(text)
        .trim_end()
        .into()
}

impl Bank {
    /// Reads a `.sf2` image.
    ///
    /// The whole image is taken as bytes, because a `no_std` crate has no file
    /// and a game has a pack. Samples are copied out of the pool into each
    /// [`Sample`](crate::Sample), with the loop points rebased, so a bank does
    /// not hold a second copy of the pool it was cut from.
    ///
    /// # Errors
    ///
    /// [`BankError::NotRiff`] and [`BankError::NotSoundFont`] when the image is
    /// not a `SoundFont` at all; [`BankError::Missing`] when one of `pdta`'s nine
    /// arrays is absent; [`BankError::RecordSize`] when one of them is not a
    /// whole number of records; and [`BankError::OutOfRange`] when a record
    /// indexes past the array it bounds.
    ///
    /// # Compressed banks
    ///
    /// A `.sf3` -- a `SoundFont` whose samples are Ogg/Vorbis streams -- parses
    /// this far and then has no audio this crate can decode, so its samples come
    /// out empty and it is silent. Decoding Vorbis is a codec, and a codec is
    /// somebody else's crate.
    pub fn parse(bytes: &[u8]) -> Result<Self, BankError> {
        if bytes.len() < 12 {
            return Err(BankError::Truncated { found: bytes.len() });
        }
        if bytes.get(..4) != Some(b"RIFF") {
            return Err(BankError::NotRiff);
        }
        if bytes.get(8..12) != Some(b"sfbk") {
            return Err(BankError::NotSoundFont);
        }
        let body = bytes.get(12..).unwrap_or_default();
        let top = chunks(body);

        let title = list(&top, *b"INFO")
            .map(|info| chunks(info))
            .and_then(|info| find(&info, *b"INAM").map(name));
        let pool = list(&top, *b"sdta")
            .map(|sdta| chunks(sdta))
            .and_then(|sdta| find(&sdta, *b"smpl"))
            .unwrap_or_default();
        let pdta = list(&top, *b"pdta").ok_or(BankError::Missing { chunk: "pdta" })?;
        let hydra = Hydra::read(&chunks(pdta))?;

        Ok(Self {
            name: title,
            presets: hydra.presets()?,
            instruments: hydra.instruments()?,
            samples: hydra.samples(pool),
        })
    }
}
