//! The table a byte golden is written as, and the one function that checks it.
//!
//! A crate that serializes anything owes the workspace a table of what its types
//! encode to, written down as literals, because that is the only thing that sees
//! a format change. The comparison lives here rather than beside each table so
//! that there is one of it: both directions, every row reported at once, and one
//! hex helper.
//!
//! [`check_digests`] and [`check_text`] are here for the same reason and not
//! because either is a format this crate defines -- they are the other two of the
//! three tables the README's blindness table calls for, and each is blind where
//! the others see. `check_digests` takes and returns plain `u64`, so nothing here
//! depends on `corvid_hash`.
mod check;
mod hex;
mod report;

pub use check::{check, check_digests, check_text};
pub use hex::{grouped, hex, unhex};
pub use report::Moved;

/// One row: a label, and what the value was recorded as -- hex for [`check`],
/// and the written text for [`check_text`].
///
/// The label is for the person reading the failure. It is not part of the
/// encoding and nothing checks it, so it should say which value the row is
/// rather than repeat the bytes.
pub type Row<'a> = (&'a str, &'a str);

/// One row of a digest table: a label, and the digest it was recorded as.
///
/// A digest table is not this crate's format -- the digest belongs to
/// `corvid_hash`, and the two encodings are independent, which is exactly why a
/// crate needs both tables. Only the comparison is shared.
pub type DigestRow<'a> = (&'a str, u64);
