//! What one seat sends every tick: its unacknowledged actions and one state
//! digest.

use alloc::{vec, vec::Vec};

use corvid_behavior::PlayerId;
use corvid_hash::Digest;
use corvid_replay::ActionLog;
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

/// The fewest rows of redundancy one datagram carries.
///
/// Four rows means a single loss needs no retransmission and a burst of three
/// still recovers on the next arrival; at fifteen ticks a second that is 266
/// milliseconds of cover. It costs four times the action bytes on every packet,
/// which for an action of a dozen bytes is a datagram under a hundred — nothing
/// next to a UDP header.
///
/// It is a floor rather than the whole window, and the difference is what makes
/// a session survive a link that goes away for a while: a datagram carries
/// every action the far end has not acknowledged, and this is the minimum it
/// carries when the far end is up to date. See [`Datagram::build`].
pub const WINDOW: usize = 4;

/// The most rows one datagram carries, however far behind the far end is.
///
/// A cap rather than "everything unacknowledged", because the window is
/// otherwise a number another machine decides — a peer that acknowledged
/// nothing for a minute would have every one of those actions in every packet.
///
/// Sixty-four rows is two seconds at thirty ticks a second, which is far past
/// what a peer can fall behind by: [`Budget::ahead`](crate::Budget) stops a peer
/// simulating more than a few ticks past the tick every seat has confirmed, and
/// [`Peer::submit`](crate::Peer::submit) will not speak for a tick twice — so a
/// peer whose link has been down for a minute has a head that stopped moving
/// ten ticks in, and the whole gap fits.
pub const CATCHUP: usize = 64;

/// What one seat sends every tick: its recent actions and one state digest.
///
/// One packet carries both, because a digest sent separately is a second packet
/// on a path that is already sending one every tick.
///
/// ```
/// # use corvid_behavior::PlayerId;
/// # use corvid_hash::Digest;
/// # use corvid_lockstep::Datagram;
/// # use corvid_time::Tick;
/// # use corvid_replay::ActionLog;
/// let mut log = ActionLog::<u8>::new(Tick::ZERO, 1);
/// log.extend_to(Tick(5))?;
/// log.set(Tick(4), PlayerId(0), 7)?;
/// log.set(Tick(5), PlayerId(0), 9)?;
///
/// // Nothing acknowledged yet, so the window is the minimum four rows ending
/// // at the head.
/// let sent = Datagram::build(
///     &log,
///     PlayerId(0),
///     Tick(5),
///     Some(Tick(1)),
///     Some(Tick(3)),
///     Tick(3),
///     Digest::from_u64(0xabc),
/// );
/// assert_eq!(sent.actions, [0, 0, 7, 9]);
/// assert_eq!(sent.head(), Tick(5));
/// assert_eq!(
///     sent.ticks().map(|(tick, _)| tick).collect::<Vec<_>>(),
///     [Tick(2), Tick(3), Tick(4), Tick(5)],
/// );
///
/// // And a far end that has acknowledged nothing gets everything the log
/// // holds, rather than the last four rows.
/// let catching_up =
///     Datagram::build(&log, PlayerId(0), Tick(5), None, None, Tick(3), Digest::ZERO);
/// assert_eq!(catching_up.first, Tick::ZERO);
/// assert_eq!(catching_up.rows(), 6);
/// # Ok::<(), corvid_replay::Refused>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Datagram<A> {
    /// Which seat sent it.
    pub seat: PlayerId,
    /// The tick the first action belongs to.
    pub first: Tick,
    /// This seat's actions for `first ..= first + actions.len() - 1`, oldest
    /// first.
    ///
    /// A vector rather than a fixed array, because how many rows go in one
    /// datagram is how many the far end has not acknowledged: a peer whose
    /// packets are all arriving sends [`WINDOW`] of them, and one whose link
    /// was down for a second sends the whole gap the moment it comes back. A
    /// fixed four made the second case unrecoverable — the rows in the gap were
    /// never sent again, and the two machines predicted them differently and
    /// stayed that way.
    pub actions: Vec<A>,
    /// The newest tick the sender has every seat's real action for, and
    /// [`None`] for a sender that is still missing one of the opening's own
    /// rows.
    ///
    /// **This is the acknowledgement**, and it is one number rather than one
    /// per seat because that is all it needs to be: a sender that has
    /// everything through `T` has this receiver's actions through `T`, whoever
    /// else it has been talking to.
    ///
    /// The [`None`] matters more than it looks. A zero here would mean "I have
    /// everything through the opening tick", which is a claim about a row a
    /// session with no input delay really does carry an action in — so a peer
    /// that had heard nothing at all would be acknowledging the one row it most
    /// needed to be sent again.
    pub heard: Option<Tick>,
    /// Which tick [`mark`](Self::mark) is the digest of.
    pub marked: Tick,
    /// This seat's state digest at [`marked`](Self::marked).
    #[serde(with = "bits")]
    pub mark: Digest,
}

impl<A: Clone + Default> Datagram<A> {
    /// Builds one from a log.
    ///
    /// The window runs from one past `acked` — what the far end says it has —
    /// to `head`, widened to at least [`WINDOW`] rows so that an ordinary
    /// single loss needs no acknowledgement to recover, narrowed to at most
    /// [`CATCHUP`] rows so that how big a packet is stays this machine's
    /// decision, and clamped to what the log still holds.
    ///
    /// Rows the log does not reach back to are `A::default()`, which is idle
    /// and is what a seat that has not acted yet means.
    #[must_use]
    pub fn build(
        log: &ActionLog<A>,
        seat: PlayerId,
        head: Tick,
        acked: Option<Tick>,
        through: Option<Tick>,
        marked: Tick,
        mark: Digest,
    ) -> Self {
        // The minimum redundancy, as a tick: `WINDOW` rows ending at the head.
        let redundant = head.saturating_sub(WINDOW as u64 - 1);
        // Everything the far end has not confirmed. A far end that has
        // acknowledged nothing wants everything the log still holds.
        let unacked = acked.map_or_else(|| log.first(), Tick::next);
        let mut first = if unacked < redundant {
            unacked
        } else {
            redundant
        };
        // And never more than the cap, nor further back than the log reaches.
        let oldest = head.saturating_sub(CATCHUP as u64 - 1);
        if first < oldest {
            first = oldest;
        }
        if first < log.first() {
            first = log.first();
        }

        let rows = head.0.saturating_sub(first.0).saturating_add(1);
        let mut actions = vec![A::default(); usize::try_from(rows).unwrap_or(WINDOW)];
        for (slot, action) in actions.iter_mut().enumerate() {
            let at = Tick(first.0.saturating_add(slot as u64));
            if let Some(recorded) = log.get(at, seat) {
                *action = recorded.clone();
            }
        }
        Self {
            seat,
            first,
            actions,
            heard: through,
            marked,
            mark,
        }
    }
}

impl<A> Datagram<A> {
    /// The newest tick this datagram carries.
    ///
    /// Derived rather than stored, so that a datagram cannot arrive claiming a
    /// head its actions do not reach — which is a thing a stranger with a
    /// socket would otherwise be able to say.
    #[must_use]
    pub const fn head(&self) -> Tick {
        Tick(
            self.first
                .0
                .saturating_add(self.actions.len().saturating_sub(1) as u64),
        )
    }

    /// How many rows it carries.
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.actions.len()
    }

    /// The tick each carried action belongs to, oldest first.
    pub fn ticks(&self) -> impl Iterator<Item = (Tick, &A)> + '_ {
        self.actions
            .iter()
            .enumerate()
            .map(move |(slot, action)| (Tick(self.first.0.saturating_add(slot as u64)), action))
    }
}

/// A digest is a number, and this is where it becomes one on the wire.
///
/// [`Digest`] carries no serde implementation of its own, for the reason
/// [`HashTrace`](corvid_replay::HashTrace) states: a digest is a number, and the
/// two places in the workspace that write a column of them down say so
/// themselves.
mod bits {
    use corvid_hash::Digest;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[allow(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde's `with` attribute calls this with a reference to the field, so the signature is the derive's rather than this function's"
    )]
    pub(super) fn serialize<S: Serializer>(mark: &Digest, out: S) -> Result<S::Ok, S::Error> {
        mark.to_u64().serialize(out)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<Digest, D::Error> {
        u64::deserialize(input).map(Digest::from_u64)
    }
}
