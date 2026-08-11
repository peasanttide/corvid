//! What a refused write to an action log says.

use core::{fmt, hash::Hash};

use corvid_behavior::PlayerId;
use corvid_time::Tick;

/// A log refused a write.
///
/// Every case here is the log declining to become something a replay could not
/// make sense of, and none of them is a failure of the simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Refused {
    /// The tick is before the log's first, which no index can address.
    Early {
        /// The tick that was asked for.
        tick: Tick,
        /// The tick the log's first row belongs to.
        first: Tick,
    },
    /// The tick has no row yet. Grow the log first.
    Beyond {
        /// The tick that was asked for.
        tick: Tick,
        /// The tick the log's first row belongs to.
        first: Tick,
        /// How many rows the log holds.
        rows: u64,
    },
    /// The seat is not one of the log's.
    Seat {
        /// The seat that was asked for.
        player: PlayerId,
        /// How many seats the log has.
        players: u16,
    },
    /// A *different* action is already confirmed there.
    ///
    /// This is the case that makes a log authoritative. Two peers that have
    /// simulated a tick against one action cannot be told afterwards that it
    /// was another one; the session either agrees or it halts.
    Confirmed {
        /// The tick that was asked for.
        tick: Tick,
        /// The seat that was asked for.
        player: PlayerId,
    },
    /// The room the request needed could not be reserved on this machine.
    Memory {
        /// How many rows the log would have had to hold.
        rows: u64,
    },
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Early { tick, first } => write!(
                f,
                "tick {tick} is before the log's first tick {first}, which no \
                 index can address"
            ),
            Self::Beyond { tick, first, rows } => write!(
                f,
                "tick {tick} has no row yet; the log holds {rows} rows from tick \
                 {first} and has to be extended before it can be written to"
            ),
            Self::Seat { player, players } => {
                write!(f, "seat {} is not one of the log's {players}", player.0)
            }
            Self::Confirmed { tick, player } => write!(
                f,
                "a different action is already confirmed for seat {} at tick \
                 {tick}: a session that has simulated a tick cannot be told it \
                 was something else",
                player.0
            ),
            Self::Memory { rows } => write!(
                f,
                "a log of {rows} rows could not be reserved on this machine"
            ),
        }
    }
}

impl core::error::Error for Refused {}
