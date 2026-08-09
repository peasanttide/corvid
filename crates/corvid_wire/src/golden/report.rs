//! What a check reports when a table and its values have stopped agreeing.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::Error;

/// A golden table and the values it was recorded from have stopped agreeing.
///
/// `Debug` prints what `Display` prints. That is deliberate rather than lazy: a
/// table is checked from a test, a test says `unwrap` or `?`, and both of those
/// print the `Debug` form -- so a derived one would put the report a person needs
/// behind a wall of field names on the one occasion they need to read it.
#[derive(Clone, PartialEq, Eq)]
pub struct Moved {
    /// What the table is a table of, for the first line of the report.
    pub(super) what: String,
    /// How many rows it has, so the report can say how much of it moved.
    pub(super) rows: usize,
    /// Everything that was wrong, in table order.
    pub(super) findings: Vec<Finding>,
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
             not a test to regenerate -- everything recorded under the old rows \
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
pub(super) enum Finding {
    /// The table and the fixture are different lengths, so nothing was compared.
    Counted { table: usize, fixture: usize },
    /// The row's literal is not whole hex bytes.
    Malformed { label: String, recorded: String },
    /// The value would not encode at all.
    Refused { label: String, why: Error },
    /// The row is something else now. `now` is the replacement literal, already
    /// written the way the table writes it -- quoted hex for a byte table, a
    /// grouped `0x...` for a digest table -- so that the report is pasted rather
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
                write!(f, "    ({label:?}, ...), would not encode: {why}")
            }
            Self::Rewritten { label, now } => write!(f, "    ({label:?}, {now}),"),
            Self::Unreadable { label, why } => write!(
                f,
                "    ({label:?}, ...), the recorded bytes no longer read back: {why}",
            ),
            Self::Changed {
                label,
                read,
                recorded_for,
            } => write!(
                f,
                "    ({label:?}, ...), the recorded bytes now read back as {read} \
                 and were recorded from {recorded_for}",
            ),
        }
    }
}
