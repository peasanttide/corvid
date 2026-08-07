//! Why a session stops, and what it says on the way out.

use alloc::{string::String, vec::Vec};
use core::fmt;

use corvid_behavior::PlayerId;
use corvid_hash::Digest;
use corvid_replay::{Refused, Unreachable};
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

/// Why a session stopped.
///
/// One error type for every method a [`Peer`](crate::Peer) has, because every
/// one of these ends the session and a caller that had to tell four of them
/// apart to decide that would be doing the same match four times.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Halt {
    /// Two peers computed different states from the same log.
    Desync(Desync),
    /// A peer contradicted an action it had already sent.
    Contradiction {
        /// Which seat told two stories.
        peer: PlayerId,
        /// The tick they disagree about.
        at: Tick,
    },
    /// A tick could not be reached from any live snapshot.
    Unreachable(Unreachable),
    /// The log refused a write.
    Refused(Refused),
    /// A level this peer could not load.
    ///
    /// The escape from the load barrier. A peer holds the simulation at the
    /// tick that asked for a level until the level is in hand, and every other
    /// peer stalls behind it inside its prediction window — so a peer that can
    /// never read the file would otherwise hang the session rather than only
    /// itself. This ends that peer instead, and the others see a departure,
    /// which they already handle.
    ///
    /// The reference is formatted rather than carried typed, so that `Halt`
    /// does not become generic over a game's level name and ripple through
    /// every caller of it.
    Unloadable {
        /// The tick that asked for it.
        at: Tick,
        /// Which level, as its `Debug`.
        reference: String,
    },
}

impl From<Desync> for Halt {
    fn from(desync: Desync) -> Self {
        Self::Desync(desync)
    }
}

impl From<Unreachable> for Halt {
    fn from(unreachable: Unreachable) -> Self {
        Self::Unreachable(unreachable)
    }
}

impl From<Refused> for Halt {
    fn from(refused: Refused) -> Self {
        Self::Refused(refused)
    }
}

impl fmt::Display for Halt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Desync(desync) => desync.fmt(f),
            Self::Contradiction { peer, at } => write!(
                f,
                "peer {} sent two different actions for tick {}, so the session \
                 has two histories and no way to choose",
                peer.0, at.0
            ),
            Self::Unreachable(unreachable) => unreachable.fmt(f),
            Self::Refused(refused) => refused.fmt(f),
            Self::Unloadable { at, reference } => write!(
                f,
                "the level {reference} asked for at tick {} could not be read, and                  the session cannot advance past a tick whose level is missing",
                at.0
            ),
        }
    }
}

impl core::error::Error for Halt {}

/// What was found, and how much of it.
///
/// The [`Display`](fmt::Display) is the report, and it is the whole reason the
/// fields are separate rather than a string: a report nobody can read is a
/// report nobody uses.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Desync {
    /// The tick the two states first differ at, rather than the tick the mark
    /// that found it arrived on.
    pub at: Tick,
    /// Whose mark disagreed.
    pub peer: PlayerId,
    /// The newest tick the two peers' marks agreed on.
    pub agreed_through: Tick,
    /// This peer's digest at [`at`](Self::at).
    pub local: Digest,
    /// The other peer's.
    pub remote: Digest,
    /// One entry per probe. Empty without the `dev` feature, which is the
    /// build that resynchronises from a full state transfer instead of
    /// bisecting.
    pub fields: Vec<FieldReport>,
    /// The first differing row, when a state transfer made it findable.
    pub first_divergent: Option<Where>,
}

/// One named subsystem, and whether the two peers agree about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FieldReport {
    /// What the game called it.
    pub probe: &'static str,
    /// Whether the two digests are the same.
    pub agrees: bool,
    /// This peer's digest of it.
    pub local: Digest,
    /// The other peer's.
    pub remote: Digest,
}

/// The first row of a named column that differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Where {
    /// What one row of the column is called — `creep`, rather than the column's
    /// own name.
    pub probe: &'static str,
    /// Which row.
    pub index: u32,
    /// Which region of the level it was in, which is what makes the row
    /// findable in a state sorted by region.
    pub region: u16,
}

/// A request for a whole state, which is what a build without `dev` does
/// instead of bisecting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Resync {
    /// Which seat is asking.
    pub seat: PlayerId,
    /// The tick it wants a state for.
    pub at: Tick,
    /// The newest tick this peer's marks agreed with the sender's, so the
    /// answer can be a state that both of them still believe in.
    pub agreed_through: Tick,
}

/// A desync is an error in the strong sense: the session cannot continue, and a
/// runtime that hands one to a caller wants it to compose with everything else
/// that can go wrong.
///
/// [`Halt`] carries the same implementation and for the same reason. Neither
/// needs `std` — `core::error::Error` is the one this crate can name — and a
/// `source` would be a second failure underneath this one, which there never
/// is: a divergence is the finding rather than a consequence of one.
impl core::error::Error for Desync {}

/// The width the report's first column is laid out at, which is the length of
/// the longest label it can print.
const LABEL: &str = "first divergent index";

impl fmt::Display for Desync {
    /// The report, exactly.
    ///
    /// ```
    /// # use corvid_behavior::PlayerId;
    /// # use corvid_hash::Digest;
    /// # use corvid_lockstep::{Desync, FieldReport, Where};
    /// # use corvid_time::Tick;
    /// let desync = Desync {
    ///     at: Tick(4127),
    ///     peer: PlayerId(2),
    ///     agreed_through: Tick(4126),
    ///     local: Digest::from_u64(0x8f21_0000_0000_0000),
    ///     remote: Digest::from_u64(0x8f20_0000_0000_0000),
    ///     fields: vec![FieldReport {
    ///         probe: "state.towers",
    ///         agrees: true,
    ///         local: Digest::ZERO,
    ///         remote: Digest::ZERO,
    ///     }],
    ///     first_divergent: None,
    /// };
    ///
    /// assert_eq!(
    ///     desync.to_string(),
    ///     "desync at tick 4127, peer 2\n  \
    ///        agreed through 4126\n  \
    ///        state.towers           agrees",
    /// );
    /// ```
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let width = self
            .fields
            .iter()
            .map(|field| field.probe.len())
            .chain(core::iter::once(LABEL.len()))
            .max()
            .unwrap_or(LABEL.len());

        writeln!(f, "desync at tick {}, peer {}", self.at.0, self.peer.0)?;
        write!(f, "  agreed through {}", self.agreed_through.0)?;
        for field in &self.fields {
            writeln!(f)?;
            if field.agrees {
                write!(f, "  {:width$}  agrees", field.probe)?;
            } else {
                write!(
                    f,
                    "  {:width$}  differs   local {} remote {}",
                    field.probe,
                    Short(field.local),
                    Short(field.remote),
                )?;
            }
        }
        if let Some(first) = self.first_divergent {
            writeln!(f)?;
            write!(
                f,
                "  {LABEL:width$}  {} {}, region {}",
                first.probe, first.index, first.region
            )?;
        }
        Ok(())
    }
}

/// A digest as the report prints one: the top sixteen bits, and an ellipsis for
/// the forty-eight that did not fit.
///
/// Enough to tell two digests apart at a glance and short enough that four of
/// them fit on a line, which is what the report is for. The whole number is in
/// [`Desync::local`] for anything that wants it.
struct Short(Digest);

impl fmt::Display for Short {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:04x}\u{2026}", self.0.to_u64() >> 48)
    }
}
