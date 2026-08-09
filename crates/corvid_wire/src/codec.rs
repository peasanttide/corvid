//! The two functions, and the one configuration they are both fixed at.

use alloc::vec::Vec;

use bincode::config::{Configuration, Limit, LittleEndian, Varint};
use bincode::error::DecodeError;
use serde::{Serialize, de::DeserializeOwned};

use crate::Error;

/// The most bytes a capture may be, in either direction.
///
/// A ceiling is here because the decoder allocates a container before it reads
/// one: `bincode` sizes a `String` or a `Vec` from its length prefix up front,
/// and with no limit configured its own guard against that is compiled out. Nine
/// bytes a peer wrote can then ask for sixteen exabytes, and asking is enough --
/// the allocation is attempted before the slice is consulted, so a ten-byte
/// packet aborts the process rather than returning an error. `tests/hostile.rs`
/// is the measurement.
///
/// The slice `decode` is handed does *not* settle this on its own. It bounds
/// what can be read; it does not bound what can be claimed, and the claim is
/// what allocates.
///
/// It applies to [`encode`] as well as to [`decode`], and that symmetry is the
/// point. A limit on the read path alone is the worse bug of the two: it writes
/// an over-large capture without complaint and refuses to read it back, so a
/// save file is lost at the moment it is needed. Here a value too large to be
/// read back is a value that will not be written down.
///
/// Two hundred and fifty-six mebibytes, against the roughly one and a quarter
/// that fifty thousand entities come to -- two hundred times the largest snapshot
/// this workspace is designed for. A capture that reaches it is a bug in the
/// caller rather than a limit worth raising.
pub const CEILING: usize = 256 * 1024 * 1024;

/// The configuration every byte in this workspace is written under.
///
/// That is what `bincode` spells `config::standard()`: little-endian, integers
/// written as variable-length quantities, an `Option` a tag byte and then its
/// payload, and no field name or type tag anywhere.
///
/// A varint spends one byte on a number below 251 and a marker plus the value's
/// own bytes above that, so it is shorter than a declared width for the small
/// numbers that dominate this format's traffic and longer for a number that
/// uses its bits. What it costs is that the width a number was declared at is no
/// longer in the bytes -- `u16(1)` and `u32(1)` are the same single byte -- so a
/// byte golden cannot see a field widen, and a digest table is what tells two
/// builds apart instead. The README's table is the whole of that argument and
/// `tests/visible.rs` is the check.
///
/// [`CEILING`] is part of it rather than a check beside it, because that is what
/// switches on `bincode`'s own pre-allocation guard: the guard is written to be
/// compiled out when no limit is configured.
///
/// It is private, and deliberately: a caller that could name the configuration
/// could pass a different one, and a second configuration is a second wire
/// format.
const WIRE: Configuration<LittleEndian, Varint, Limit<CEILING>> =
    bincode::config::standard().with_limit::<CEILING>();

/// Writes `value` down.
///
/// ```
/// # fn main() -> Result<(), corvid_wire::Error> {
/// // A small number is one byte whatever it was declared as, so the two lines
/// // below are the same byte -- the width is not in the bytes.
/// assert_eq!(corvid_wire::encode(&1_u16)?, [0x01]);
/// assert_eq!(corvid_wire::encode(&1_u32)?, [0x01]);
///
/// // Above 250 a marker names how many bytes follow, least significant first.
/// assert_eq!(corvid_wire::encode(&251_u16)?, [0xfb, 0xfb, 0x00]);
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// [`Error::Wrote`] if the value's `Serialize` refused, or asked for something
/// this format cannot write. The shape that reaches this in practice is
/// `#[serde(flatten)]`, which serializes its fields as a map of unknown length
/// and is refused as `Serde(SequenceMustHaveLength)`. A type whose *reader*
/// needs the names -- an untagged enum -- writes down perfectly well and fails in
/// [`decode`] instead.
///
/// [`Error::TooLarge`] if the value came to more than [`CEILING`] bytes, which
/// is refused here so that it is never written to a file that cannot be read
/// back.
pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, Error> {
    let written = bincode::serde::encode_to_vec(value, WIRE).map_err(|why| Error::wrote(&why))?;
    // Checked after the fact rather than during, because `bincode` applies a
    // configured limit to the read path only. The bytes exist by now and are
    // dropped, which costs the memory once for a value the caller already held --
    // where the same length arriving from a peer would have cost it before
    // anything had been read.
    if written.len() > CEILING {
        return Err(Error::TooLarge {
            wrote: Some(written.len()),
        });
    }
    Ok(written)
}

/// Reads a value back, and refuses bytes that have more in them than the value.
///
/// The refusal is the point. A capture that grew a field is a byte string whose
/// prefix still parses as the old type, so a decoder that stopped when it had
/// enough would hand back a value that is *not* what was recorded and report
/// success.
///
/// ```
/// # fn main() -> Result<(), corvid_wire::Error> {
/// let bytes = corvid_wire::encode(&(1_u16, 2_u16))?;
/// assert_eq!(corvid_wire::decode::<(u16, u16)>(&bytes)?, (1, 2));
///
/// // The same bytes with one more on the end: the pair is still in there, and
/// // that is exactly why reading it would be the wrong answer.
/// let mut grown = bytes.clone();
/// grown.push(0);
/// assert!(matches!(
///     corvid_wire::decode::<(u16, u16)>(&grown),
///     Err(corvid_wire::Error::Trailing { used: 2, len: 3 }),
/// ));
/// # Ok(())
/// # }
/// ```
///
/// A count a peer wrote is checked against [`CEILING`] before it is acted on,
/// not against the length of `bytes`. The slice bounds what can be *read* and
/// not what can be *claimed*, and a container is sized from its count before a
/// byte of it is read -- so ten bytes asking for sixteen exabytes has to be
/// refused on the strength of the number alone.
///
/// # Errors
///
/// [`Error::Read`] if the bytes do not decode as `T`. That covers a capture
/// that ran out -- including one whose length prefix claims more than the slice
/// holds, which is refused as `UnexpectedEnd` -- and a type whose reader needs
/// field names, such as an untagged enum, which is refused as
/// `Serde(AnyNotSupported)`.
///
/// [`Error::TooLarge`] if the bytes ask to allocate more than [`CEILING`],
/// refused before anything is allocated.
///
/// [`Error::Trailing`] if the bytes decode and are longer than the value needed.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    let (value, used) =
        bincode::serde::decode_from_slice(bytes, WIRE).map_err(|why| match why {
            // The one variant worth telling apart from a corrupt capture: it says
            // the bytes were well-formed enough to name a size, and that the size
            // was refused. Matching on it does not leak which encoder produced it,
            // because what is carried out is this crate's own variant.
            DecodeError::LimitExceeded => Error::TooLarge { wrote: None },
            other => Error::read(&other),
        })?;
    if used == bytes.len() {
        Ok(value)
    } else {
        Err(Error::Trailing {
            used,
            len: bytes.len(),
        })
    }
}
