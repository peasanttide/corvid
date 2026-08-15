//! What a golden comparison refuses with, and how it reads.
//!
//! The seam against `goldens.rs` is that nothing here touches a file: the
//! comparison walks the two directories and hands what it found to these
//! types, which are the whole of what a failed check prints.

use std::{fmt, io, path::PathBuf};

use crate::goldens::{BLESS, EXTENSION};

/// One golden that no longer says what the capture holds.
#[derive(Clone, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[error("{what}  {how}")]
pub struct Finding {
    /// Which file, as a path relative to the capture directory -- so
    /// `goldens/audio/42.hex` is reported as `audio/42`.
    pub what: String,
    /// How it disagrees.
    pub how: How,
}

/// The three ways a golden and a capture fail to be the same bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum How {
    /// The capture holds no file for this golden.
    ///
    /// The one difference blessing cannot fix: there is nothing to record it
    /// from. A run that stopped producing a file somebody froze lands here, and
    /// so does a golden whose name no longer matches what the capture calls it.
    #[error("the capture holds no file to compare it with")]
    Absent,
    /// The golden is there and is not whole hex bytes.
    #[error("the recorded golden is not whole hex bytes")]
    Malformed,
    /// The bytes moved.
    #[error("{}", moved(*at, *recorded, *captured))]
    Moved {
        /// The first byte offset the two disagree at, or the shorter length
        /// when one is a prefix of the other.
        at: usize,
        /// How many bytes the golden holds.
        recorded: usize,
        /// How many the capture holds.
        captured: usize,
    },
}

impl How {
    /// Whether blessing can record this one from the capture.
    pub(crate) const fn is_rewritable(self) -> bool {
        match self {
            Self::Absent => false,
            Self::Malformed | Self::Moved { .. } => true,
        }
    }
}

/// Two lengths and an offset, as the one sentence that reads for both.
///
/// Equal lengths are the common case and "8 bytes recorded against 8
/// captured" reads as though something about the lengths were the finding, so
/// the two are said once when they agree.
fn moved(at: usize, recorded: usize, captured: usize) -> String {
    if recorded == captured {
        format!("{recorded} bytes either way, first differing at offset {at}")
    } else {
        format!(
            "{recorded} bytes recorded against {captured} captured, first differing at offset {at}"
        )
    }
}

/// A capture and its goldens are not the same bytes.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Mismatch {
    /// There are no goldens, so nothing was compared.
    #[error(
        "{} holds no *.{EXTENSION} goldens, so this comparison had nothing to say: create a file \
         per capture file to freeze and set {BLESS} to record them",
        goldens.display()
    )]
    Unfrozen {
        /// The directory that holds none.
        goldens: PathBuf,
    },
    /// A file or a directory could not be read or written. Nothing else was
    /// compared.
    #[error("{}: {why}", path.display())]
    Unreadable {
        /// Which one. `io::Error` does not carry the path it was about.
        path: PathBuf,
        /// Why not.
        #[source]
        why: io::Error,
    },
    /// Goldens moved, and nothing was written.
    #[error(
        "{} {} in {} no longer {} what this capture holds:\n{}if that is deliberate, {BLESS}=1 \
         records them from this capture, and it is the only sanctioned way to change one",
        findings.len(),
        counted(findings.len()).0,
        goldens.display(),
        counted(findings.len()).1,
        table(findings)
    )]
    Moved {
        /// Where they live.
        goldens: PathBuf,
        /// Every one that moved, in path order.
        findings: Vec<Finding>,
    },
    /// [`BLESS`] was set, and these were rewritten from the capture.
    #[error(
        "{BLESS} was set: {} {} in {} no longer said what this capture holds, and every one the \
         capture had a file for was rewritten from it:\n{}run again to compare against what was \
         just recorded",
        findings.len(),
        counted(findings.len()).0,
        goldens.display(),
        table(findings)
    )]
    Rewritten {
        /// Where they live.
        goldens: PathBuf,
        /// Every one that moved. Each was rewritten except the ones the capture
        /// had no file for, which nothing can be recorded from.
        findings: Vec<Finding>,
    },
}

/// The noun and the verb for a count of findings.
///
/// "1 of the goldens no longer say" is what a report reads like when nobody
/// writes this, and a report that reads like a bug is one a person distrusts at
/// the moment they most need to trust it.
const fn counted(findings: usize) -> (&'static str, &'static str) {
    if findings == 1 {
        ("golden", "says")
    } else {
        ("goldens", "say")
    }
}

/// Every finding, one to a line, with the names in a column.
///
/// All of them and not the first: a deliberate format change moves every golden
/// at once, and a report that stopped at the first would show a deliberate
/// change and an accidental one identically.
fn table(findings: &[Finding]) -> String {
    let width = findings
        .iter()
        .map(|finding| finding.what.len())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for finding in findings {
        use fmt::Write as _;
        let _ = writeln!(out, "    {:width$}  {}", finding.what, finding.how);
    }
    out
}
