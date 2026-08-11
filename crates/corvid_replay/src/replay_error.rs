//! The three ways a capture can refuse to become a session.
//!
//! Together because they are read together: a caller matching on [`Load`] is
//! deciding what to tell a player, and the two below it are the detail that
//! answer names. Apart from the structures they describe because an error enum
//! is prose about failure, and interleaving the two makes both harder to
//! follow.

use core::hash::Hash;

use corvid_hash::Digest;
use corvid_time::Tick;

/// A capture could not be turned into a session this build can replay.
#[derive(Clone, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum Load {
    /// The bytes are not a session, with the encoder's reason.
    #[error("the bytes are not a session: {0}")]
    Bytes(corvid_wire::Error),
    /// The capture was recorded by a build that describes its types
    /// differently.
    ///
    /// This is the refusal the schema exists for: replaying a session under a
    /// build whose `State` means something else produces a state that is wrong
    /// without being detectably wrong, and the first thing that notices is a
    /// peer, later, disagreeing about a digest.
    #[error(
        "this capture was recorded by a build describing itself as {recorded} and \
         this build describes itself as {running}: replaying it would not \
         reproduce the session it recorded"
    )]
    Schema {
        /// What the capture says the build that wrote it was.
        recorded: Digest,
        /// What this build says it is.
        running: Digest,
    },
    /// The capture's own parts disagree about the session they describe.
    #[error("the capture's parts disagree: {0}")]
    Shape(Shape),
}

/// A session would not forget to a tick.
///
/// Both cases are a tick outside the stretch the session covers, and neither is
/// about how much of it is worth keeping: forgetting to a tick a session already
/// opens at is legal and does nothing, which is what lets a runtime call it on a
/// schedule without first asking where it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum Forget {
    /// Before the opening. There is nothing there to forget.
    #[error(
        "tick {tick} is before the session's opening tick {first}, so there is \
         nothing before it to forget"
    )]
    Early {
        /// The tick that was asked for.
        tick: Tick,
        /// The tick the session opens on.
        first: Tick,
    },
    /// Past the last tick the log reaches, so the session has no state there to
    /// be told about -- and forgetting to it would drop rows whose states nobody
    /// has computed yet.
    #[error(
        "tick {tick} is past tick {last}, which is as far as this session's log \
         reaches: forgetting to it would drop rows whose states nothing has \
         computed"
    )]
    Beyond {
        /// The tick that was asked for.
        tick: Tick,
        /// The latest tick the session's log reaches.
        last: Tick,
    },
}

/// Which of a capture's parts disagreed with which.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum Shape {
    /// The log starts at a different tick than the opening.
    #[error(
        "the log's first row is tick {log} and the session opens at {opening}, so \
         every row would be read against the wrong tick"
    )]
    LogStart {
        /// The log's first tick.
        log: Tick,
        /// The opening's.
        opening: Tick,
    },
    /// The trace starts at a different tick than the opening.
    #[error(
        "the trace's first mark is tick {trace} and the session opens at \
         {opening}, so every mark would be compared against the wrong tick"
    )]
    TraceStart {
        /// The trace's first tick.
        trace: Tick,
        /// The opening's.
        opening: Tick,
    },
    /// The roster names more seats than a [`PlayerId`](corvid_behavior::PlayerId) can address.
    ///
    /// A seat number is a `u16`, so a roster of more than sixty-five thousand
    /// has seats no action can be attributed to. It is the one shape check that
    /// is about a type's range rather than about two parts disagreeing.
    #[error(
        "the roster names {seats} seats and a seat number is a u16, so everything \
         past {} has no action that could be attributed to it",
        u16::MAX
    )]
    Roster {
        /// How many seats the roster names.
        seats: usize,
    },
    /// The log's rows are not as wide as the roster.
    #[error(
        "a row of the log holds {log} seats and the roster names {roster}, so \
         every row after the first would be read against the wrong seats"
    )]
    Width {
        /// How many seats a row of the log holds.
        log: u16,
        /// How many the roster names.
        roster: u16,
    },
    /// The log's entries stop partway through a row.
    ///
    /// This is the one that looks like a few wasted bytes and is not.
    /// [`ActionLog::ticks`](crate::ActionLog::ticks) counts whole rows, so the
    /// entries past the last one are unreachable through every accessor *while
    /// the log stays this length* -- and they are not off to one side, they are
    /// the front of the next row. The first
    /// [`extend_to`](crate::ActionLog::extend_to) makes that row exist, and it
    /// arrives already holding those entries, with whatever confirmation bits
    /// the capture set for them. From that tick on the session simulates
    /// actions nobody recorded for seats nobody played, and the peers sending
    /// the real ones are turned away with
    /// [`Refused::Confirmed`](crate::Refused::Confirmed).
    #[error(
        "the log holds {entries} entries in rows of {players}, which is not a \
         whole number of rows: the entries past the last whole row are the \
         front of the next one the log grows, where they would be read as \
         actions this capture never recorded"
    )]
    Ragged {
        /// How many entries the log holds.
        entries: usize,
        /// How many seats wide a row is.
        players: u16,
    },
    /// The log's confirmation bitmap is not as long as its entries need.
    ///
    /// This is the one that costs the log its authority rather than its
    /// indexing. A bit past the end of the bitmap reads as zero, so every entry
    /// it does not cover is *unconfirmed* -- and an unconfirmed entry can be
    /// written to. A capture that arrived a byte short would let a peer rewrite
    /// actions the session has already agreed on and simulated, one at a time,
    /// with no refusal anywhere.
    #[error(
        "the log's confirmation bitmap holds {bytes} bytes and its {entries} \
         entries need {needed}, so the entries it does not cover read as \
         unconfirmed and anything could be written over what the session \
         already agreed on"
    )]
    Confirmations {
        /// How many bytes the bitmap holds.
        bytes: usize,
        /// How many the entries need.
        needed: usize,
        /// How many entries the log holds.
        entries: usize,
    },
}
