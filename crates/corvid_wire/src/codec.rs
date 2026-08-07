//! The two functions, and the one configuration they are both fixed at.

use alloc::vec::Vec;

use bincode::config::{Configuration, LittleEndian, Varint};
use serde::{Serialize, de::DeserializeOwned};

use crate::Error;

/// The configuration every byte in this workspace is written under.
///
/// That is what `bincode` spells `config::standard()`: little-endian, integers
/// written as variable-length quantities, an `Option` a tag byte and then its
/// payload, and no field name or type tag anywhere.
///
/// A varint spends one byte on a number below 251 and a marker plus the value's
/// own bytes above that, so it is shorter than a declared width for the small
/// numbers that dominate this format's traffic and longer for a number that
/// uses its bits. The count in front of a sequence is the clearest case: it is a
/// `u64`, it is almost always small, and it is paid once per list. What it costs
/// is that the width a number was declared at is no longer in the bytes —
/// `u16(1)` and `u32(1)` are the same single byte — so a byte golden cannot see
/// a field widen and [`Schema`](../../corvid_replay/struct.Schema.html) is what
/// tells two builds apart instead. The README's table is the whole of that
/// argument and `tests/visible.rs` is the check.
///
/// It is private, and deliberately: a caller that could name the configuration
/// could pass a different one, and a second configuration is a second wire
/// format.
const WIRE: Configuration<LittleEndian, Varint> = bincode::config::standard();

/// Writes `value` down.
///
/// ```
/// # fn main() -> Result<(), corvid_wire::Error> {
/// // A small number is one byte whatever it was declared as, so the two lines
/// // below are the same byte — the width is not in the bytes.
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
/// needs the names — an untagged enum — writes down perfectly well and fails in
/// [`decode`] instead.
pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, Error> {
    bincode::serde::encode_to_vec(value, WIRE).map_err(|why| Error::wrote(&why))
}

/// Reads a value back, and refuses bytes that have more in them than the value.
///
/// The refusal is the point. A capture that grew a field is a byte string whose
/// prefix still parses as the old type, so a decoder that stopped when it had
/// enough would hand back a value that is *not* what was recorded and report
/// success. A save file from a newer build, a snapshot from a peer that is one
/// commit ahead, a golden row that was regenerated against a changed type — all
/// three arrive as a longer byte string with a readable prefix, and all three
/// have to fail.
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
/// The argument `bytes` is a slice, and that is the whole of this function's
/// answer to a length prefix a peer wrote. A sequence is a count and then its
/// elements, and the count arrives before the elements do — but the
/// elements have to arrive from *this slice*, whose length the caller already
/// knows, so a count larger than the slice runs out of bytes and is an
/// [`Error::Read`] rather than an allocation. `serde` also caps the capacity it
/// reserves up front on the strength of a sequence's size hint, so neither half
/// acts on the number. `tests/trailing.rs` shows the consequence: a hostile
/// `u64::MAX` count and an honest count of two produce the same error over the
/// same truncated bytes, which a decoder that had believed either number could
/// not do.
///
/// A transport that reads from a socket instead has no such slice, and there a
/// bound belongs — but it belongs where the bytes are being pulled in, not here.
/// See the note in this crate's README.
///
/// # Errors
///
/// [`Error::Read`] if the bytes do not decode as `T`. That covers a capture
/// that ran out — including one whose length prefix claims far more than the
/// slice holds, which is refused as `UnexpectedEnd` — and a type whose reader
/// needs field names, such as an untagged enum, which is refused as
/// `Serde(AnyNotSupported)`. [`Error::Trailing`] if the bytes decode and are
/// longer than the value needed.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    let (value, used) =
        bincode::serde::decode_from_slice(bytes, WIRE).map_err(|why| Error::read(&why))?;
    if used == bytes.len() {
        Ok(value)
    } else {
        Err(Error::Trailing {
            used,
            len: bytes.len(),
        })
    }
}
