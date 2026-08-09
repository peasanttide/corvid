//! The table a byte golden is written as, and the one function that checks it.
//!
//! A crate that serializes anything owes the workspace a table of what its types
//! encode to, written down as literals, because that is the only thing that sees
//! a format change. The comparison lives here rather than beside each table so
//! that there is one of it: both directions, every row reported at once, and one
//! hex helper.
//!
//! [`check_digests`] and [`check_text`] are here for the same reason and not
//! because either is a format this crate defines — they are the other two of the
//! three tables the README's blindness table calls for, and each is blind where
//! the others see. `check_digests` takes and returns plain `u64`, so nothing here
//! depends on `corvid_hash`.

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::fmt;

use serde::{Serialize, de::DeserializeOwned};

use crate::{Error, decode, encode};

/// One row: a label, and what the value was recorded as — hex for [`check`],
/// and the written text for [`check_text`].
///
/// The label is for the person reading the failure. It is not part of the
/// encoding and nothing checks it, so it should say which value the row is
/// rather than repeat the bytes.
pub type Row<'a> = (&'a str, &'a str);

/// One row of a digest table: a label, and the digest it was recorded as.
///
/// A digest table is not this crate's format — the digest belongs to
/// `corvid_hash`, and the two encodings are independent, which is exactly why a
/// crate needs both tables. Only the comparison is shared.
pub type DigestRow<'a> = (&'a str, u64);

/// Compares a labelled fixture against its recorded bytes, both ways round.
///
/// The two directions are two different promises and a table is worth little
/// without both. That today's encoder writes the recorded bytes is what says a
/// capture taken now will still be readable by the build that recorded them.
/// That the recorded bytes still read back as the value they were recorded from
/// is what says a capture taken *then* is readable now — which is the direction
/// a game actually depends on when it opens a save file, and the one a decoder
/// is under no obligation to satisfy merely because the encoder is unchanged.
///
/// Everything that moved is reported at once, formatted the way a table is
/// written, so that a deliberate format change is one paste and an accidental
/// one shows its whole shape rather than its first row.
///
/// ```
/// use corvid_wire::golden::{Row, check};
///
/// const GOLDEN: &[Row<'_>] = &[("one", "01"), ("two", "02")];
///
/// check("u16", GOLDEN, &[1_u16, 2]).unwrap();
/// assert!(check("u16", GOLDEN, &[1_u16, 3]).is_err());
/// ```
///
/// # Errors
///
/// [`Moved`] if the table and the fixture are different lengths, if any row's
/// literal is not whole hex bytes, if any value no longer encodes to its
/// recorded row, or if any recorded row no longer reads back as the value it
/// was recorded from.
pub fn check<T>(what: &str, table: &[Row<'_>], values: &[T]) -> Result<(), Moved>
where
    T: Serialize + DeserializeOwned + PartialEq + fmt::Debug,
{
    if table.len() != values.len() {
        return Err(Moved {
            what: what.to_string(),
            rows: table.len(),
            findings: alloc::vec![Finding::Counted {
                table: table.len(),
                fixture: values.len(),
            }],
        });
    }

    let mut findings = Vec::new();
    for (&(label, recorded), value) in table.iter().zip(values) {
        let label = label.to_string();
        let written = match encode(value) {
            Ok(written) => written,
            Err(why) => {
                findings.push(Finding::Refused { label, why });
                continue;
            }
        };
        let Some(bytes) = unhex(recorded) else {
            findings.push(Finding::Malformed {
                label,
                recorded: recorded.to_string(),
            });
            continue;
        };

        if written == bytes {
            match decode::<T>(&bytes) {
                Ok(read) if &read == value => {}
                Ok(read) => findings.push(Finding::Changed {
                    label,
                    read: format!("{read:?}"),
                    recorded_for: format!("{value:?}"),
                }),
                Err(why) => findings.push(Finding::Unreadable { label, why }),
            }
        } else {
            findings.push(Finding::Rewritten {
                label,
                now: format!("{:?}", hex(&written)),
            });
        }
    }

    if findings.is_empty() {
        Ok(())
    } else {
        Err(Moved {
            what: what.to_string(),
            rows: table.len(),
            findings,
        })
    }
}

/// Compares a labelled fixture's digests against the ones recorded for it.
///
/// One direction only, because a digest has only one: there is nothing to read a
/// digest back into, which is the difference between this and [`check`]. What a
/// digest table catches is an encoding that still tells two values apart and
/// tells them apart *differently* than when the row was recorded — invisible to
/// any test that compares one of today's outputs against another of today's
/// outputs.
///
/// Everything that moved is reported at once, as paste-ready literals in the
/// grouped-hex form these tables are written in, for the same reason as
/// [`check`]: a deliberate change moves every row.
///
/// ```
/// use corvid_wire::golden::{DigestRow, check_digests};
///
/// const GOLDEN: &[DigestRow<'_>] = &[
///     ("the opening tick", 0x7383_3581_a38e_f3cd),
///     ("the tick after it", 0x3178_2188_0dd5_d02b),
/// ];
///
/// check_digests("Trace", GOLDEN, &[0x7383_3581_a38e_f3cd, 0x3178_2188_0dd5_d02b]).unwrap();
///
/// let moved = check_digests("Trace", GOLDEN, &[0x7383_3581_a38e_f3cd, 0]).unwrap_err();
/// assert!(moved.to_string().contains("0x0000_0000_0000_0000"), "{moved}");
/// ```
///
/// # Errors
///
/// [`Moved`] if the table and the fixture are different lengths, or if any
/// value no longer digests to its recorded row.
pub fn check_digests(what: &str, table: &[DigestRow<'_>], digests: &[u64]) -> Result<(), Moved> {
    if table.len() != digests.len() {
        return Err(Moved {
            what: what.to_string(),
            rows: table.len(),
            findings: alloc::vec![Finding::Counted {
                table: table.len(),
                fixture: digests.len(),
            }],
        });
    }

    let findings: Vec<Finding> = table
        .iter()
        .zip(digests)
        .filter(|((_, recorded), actual)| recorded != *actual)
        .map(|((label, _), actual)| Finding::Rewritten {
            label: (*label).to_string(),
            now: grouped(*actual),
        })
        .collect();

    if findings.is_empty() {
        Ok(())
    } else {
        Err(Moved {
            what: what.to_string(),
            rows: table.len(),
            findings,
        })
    }
}

/// Compares a labelled fixture's written text against what was recorded for it.
///
/// This is the helper a *self-describing* table uses, and it is here for the
/// same reason [`check_digests`] is: the comparison is about recorded rows and
/// not about a format. Nothing in this crate writes text.
///
/// What it catches that a byte table cannot is a field or a variant renamed, two
/// same-typed fields swapped when their recorded values are equal, and a field
/// added that writes no bytes — this encoding carries no names, so all four are
/// invisible to it. `tests/blind.rs` measures the last two.
///
/// ```
/// use corvid_wire::golden::{Row, check_text};
///
/// const GOLDEN: &[Row<'_>] = &[("the origin", r#"{"x":0,"y":0}"#)];
///
/// let written = vec![r#"{"x":0,"y":0}"#.to_string()];
/// check_text("Point", GOLDEN, &written).unwrap();
///
/// let renamed = vec![r#"{"across":0,"y":0}"#.to_string()];
/// assert!(check_text("Point", GOLDEN, &renamed).is_err());
/// ```
///
/// # Errors
///
/// [`Moved`] if the table and the fixture are different lengths, or if any
/// value no longer writes down as its recorded row.
pub fn check_text(what: &str, table: &[Row<'_>], written: &[String]) -> Result<(), Moved> {
    if table.len() != written.len() {
        return Err(Moved {
            what: what.to_string(),
            rows: table.len(),
            findings: alloc::vec![Finding::Counted {
                table: table.len(),
                fixture: written.len(),
            }],
        });
    }

    let findings: Vec<Finding> = table
        .iter()
        .zip(written)
        .filter(|((_, recorded), actual)| recorded != actual)
        .map(|((label, _), actual)| Finding::Rewritten {
            label: (*label).to_string(),
            now: raw(actual),
        })
        .collect();

    if findings.is_empty() {
        Ok(())
    } else {
        Err(Moved {
            what: what.to_string(),
            rows: table.len(),
            findings,
        })
    }
}

/// A digest as a golden table writes them: sixteen hex digits in groups of four.
#[must_use]
pub fn grouped(digest: u64) -> String {
    let mut text = String::with_capacity(23);
    text.push_str("0x");
    for group in 0..4 {
        if group != 0 {
            text.push('_');
        }
        for nibbles in 0..4 {
            let shift = 60 - group * 16 - nibbles * 4;
            // The mask leaves one nibble, so the narrowing is exact and
            // `u8::try_from` cannot fail. Saying it this way rather than with a
            // cast keeps the workspace's cast lints meaningful here.
            let digit = u8::try_from((digest >> shift) & 0xf).unwrap_or(0);
            text.push(nibble(digit));
        }
    }
    text
}

/// Bytes as a golden table writes them: two lowercase hex digits each.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(nibble(byte >> 4));
        text.push(nibble(byte & 0x0f));
    }
    text
}

/// The inverse of [`hex`], so a recorded row can be decoded rather than only
/// compared.
///
/// Whitespace is ignored and means nothing, which is what lets a long row be
/// written in groups. [`None`] when what is left is not whole pairs of hex
/// digits — a row that has lost a character is a mistake in the table rather
/// than a wire-format break, and the two should not look alike.
#[must_use]
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    digits
        .chunks_exact(2)
        .map(|pair| {
            let high = value(*pair.first()?)?;
            let low = value(*pair.get(1)?)?;
            Some((high << 4) | low)
        })
        .collect()
}

/// A string as a Rust raw literal that actually parses.
///
/// A raw string is terminated by a quote followed by as many hashes as opened
/// it, so the number of hashes has to exceed the longest run already inside the
/// text. JSON is exactly where this bites: a recorded row for a struct with a
/// `String` field holding `"#` closes a `r#"…"#` early, and the report a person
/// was told to paste does not compile.
fn raw(text: &str) -> String {
    let mut longest = 0_usize;
    let mut run: Option<usize> = None;
    for character in text.chars() {
        run = match (run, character) {
            (Some(hashes), '#') => Some(hashes + 1),
            (_, '"') => Some(0),
            _ => None,
        };
        if let Some(hashes) = run {
            longest = longest.max(hashes + 1);
        }
    }

    let mut literal = String::with_capacity(text.len() + 2 * longest + 4);
    literal.push('r');
    for _ in 0..longest {
        literal.push('#');
    }
    literal.push('"');
    literal.push_str(text);
    literal.push('"');
    for _ in 0..longest {
        literal.push('#');
    }
    literal
}

/// One hex digit's character.
fn nibble(value: u8) -> char {
    if value < 10 {
        char::from(b'0' + value)
    } else {
        char::from(b'a' + value - 10)
    }
}

/// One hex digit's value, and [`None`] for anything that is not one.
const fn value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

/// A golden table and the values it was recorded from have stopped agreeing.
///
/// `Debug` prints what `Display` prints. That is deliberate rather than lazy: a
/// table is checked from a test, a test says `unwrap` or `?`, and both of those
/// print the `Debug` form — so a derived one would put the report a person needs
/// behind a wall of field names on the one occasion they need to read it.
#[derive(Clone, PartialEq, Eq)]
pub struct Moved {
    /// What the table is a table of, for the first line of the report.
    what: String,
    /// How many rows it has, so the report can say how much of it moved.
    rows: usize,
    /// Everything that was wrong, in table order.
    findings: Vec<Finding>,
}

impl Moved {
    /// How many rows had something wrong with them.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.findings.len()
    }
}

impl fmt::Display for Moved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} of {} recorded rows moved, which is a wire-format break and \
             not a test to regenerate — everything recorded under the old rows \
             now means something else:",
            self.what,
            self.findings.len(),
            self.rows,
        )?;
        for finding in &self.findings {
            write!(f, "\n{finding}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Moved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl core::error::Error for Moved {}

/// One thing that was wrong with one row.
#[derive(Clone, PartialEq, Eq)]
enum Finding {
    /// The table and the fixture are different lengths, so nothing was compared.
    Counted { table: usize, fixture: usize },
    /// The row's literal is not whole hex bytes.
    Malformed { label: String, recorded: String },
    /// The value would not encode at all.
    Refused { label: String, why: Error },
    /// The row is something else now. `now` is the replacement literal, already
    /// written the way the table writes it — quoted hex for a byte table, a
    /// grouped `0x…` for a digest table — so that the report is pasted rather
    /// than transcribed.
    Rewritten { label: String, now: String },
    /// The recorded bytes no longer decode.
    Unreadable { label: String, why: Error },
    /// The recorded bytes decode to a different value than they were recorded
    /// from, which is the worst of these: a capture that still loads, as
    /// something it never was.
    Changed {
        label: String,
        read: String,
        recorded_for: String,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Counted { table, fixture } => write!(
                f,
                "    the table has {table} rows and the fixture has {fixture}",
            ),
            Self::Malformed { label, recorded } => write!(
                f,
                "    ({label:?}, {recorded:?}), is not whole bytes of hex",
            ),
            Self::Refused { label, why } => {
                write!(f, "    ({label:?}, …), would not encode: {why}")
            }
            Self::Rewritten { label, now } => write!(f, "    ({label:?}, {now}),"),
            Self::Unreadable { label, why } => write!(
                f,
                "    ({label:?}, …), the recorded bytes no longer read back: {why}",
            ),
            Self::Changed {
                label,
                read,
                recorded_for,
            } => write!(
                f,
                "    ({label:?}, …), the recorded bytes now read back as {read} \
                 and were recorded from {recorded_for}",
            ),
        }
    }
}
