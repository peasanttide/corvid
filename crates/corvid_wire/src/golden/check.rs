//! The comparison itself: a table against the values it was recorded from.

use alloc::format;
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::fmt;

use serde::{Serialize, de::DeserializeOwned};

use super::hex::{grouped, hex, raw, unhex};
use super::report::{Finding, Moved};
use super::{DigestRow, Row};
use crate::{decode, encode};

/// Compares a labelled fixture against its recorded bytes, both ways round.
///
/// The two directions are two different promises and a table is worth little
/// without both. That today's encoder writes the recorded bytes is what says a
/// capture taken now will still be readable by the build that recorded them.
/// That the recorded bytes still read back as the value they were recorded from
/// is what says a capture taken *then* is readable now -- which is the direction
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
/// tells them apart *differently* than when the row was recorded -- invisible to
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
/// added that writes no bytes -- this encoding carries no names, so all four are
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
