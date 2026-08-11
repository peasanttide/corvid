//! One action per player per tick, dense, with the confirmed entries marked.
//!
//! The type, how it is written down, and everything that only reads it. What
//! *writes* to a log is [`write`](self::write), and the refusal a
//! write can answer with is [`Refused`](crate::Refused) -- split off because
//! reading a log is the common path and the rules about overwriting a confirmed
//! entry are a subject of their own.

mod encode;
mod error;
mod write;

pub use error::Refused;

use alloc::vec::Vec;

use corvid_behavior::PlayerId;
use corvid_time::Tick;
use serde::Serialize;

/// The actions of a session, one per seat per tick, laid out flat.
///
/// The entry for `tick` and `player` lives at index
/// `(tick - first) * players + player`, so a row is contiguous and a lookup is
/// arithmetic rather than a search. Nothing is optional: an entry inside the
/// recorded range that nobody has [`set`](Self::set) holds `A::default()`,
/// which is what "this player did nothing" already means in `corvid_behavior`.
/// That is the whole reason the log is dense rather than sparse -- a game never
/// asks whether an action is present, so neither does a replay.
///
/// # Confirmed, and why a bit is kept for it
///
/// "Absent" and "confirmed idle" are the same eight bytes and are not the same
/// thing. A peer that has confirmed `Action::default()` for a tick and then
/// sends something else for the same tick is contradicting itself, and a log
/// that could not tell the two apart would accept the contradiction silently.
/// So a bit per entry records whether anybody has ever set it, and
/// [`set`](Self::set) reads that bit rather than comparing against the default.
///
/// # The generation, and what a snapshot is keyed to
///
/// A snapshot is a tick and a state and records no log, so something has to say
/// whether the log a state was simulated against is still this one. That is the
/// **generation**: [`set`](Self::set) counts every write that *changes* a stored
/// action, and [`generation_at`](Self::generation_at) reports how many of those
/// have landed on rows strictly before a given tick -- which is exactly the set
/// of rows the state at that tick was computed from. A snapshot records the
/// number it was taken under and [`Session::seek`](crate::Session::seek) ignores
/// it when the log no longer agrees, so a correction cannot be quietly left out
/// of an answer.
///
/// "Strictly before" is the whole of the rule and is worth being exact about.
/// The state at tick `T` is what simulating the rows at `first` through `T - 1`
/// produces, and the row at `T` is the one that carries it on to the state at
/// `T + 1`. So the state at `T` does not depend on row `T`: a correction there
/// leaves it exactly as it was and invalidates every state after it. The rows a
/// snapshot at `T` is keyed to are therefore `first ..= T - 1`, and a snapshot
/// at `first` is keyed to none of them.
///
/// The looser rule -- count the row at `T` as well -- reads like a safe
/// over-approximation and is not one. An over-approximation would throw away
/// some entries that were still good and keep the rest; this one keeps nothing
/// at all. Ordinary forward play keeps the state at `S` and only then learns
/// what the seats did on `S`, so writing row `S` would invalidate the snapshot
/// taken at `S` moments earlier -- on every tick, for every entry, in the case
/// the ring exists for. What it approximates is not a smaller ring but an empty
/// one, with the bookkeeping still paid for, and every seek back to the opening.
/// `tests/seek.rs` runs that case, and `tests/log.rs` pins the boundary entry by
/// entry.
///
/// What this does not cover is a log **replaced** rather than corrected. Two
/// logs built separately have no shared history to compare, so a `Session` whose
/// `log` field is assigned a different log is a case for
/// [`Snapshots::clear`](crate::Snapshots::clear) or
/// [`Snapshots::discard_from`](crate::Snapshots::discard_from), which stay the
/// caller's precise tools.
///
/// The count is kept per row, which costs eight bytes a row beside the actions
/// themselves. A correction is written into the row it lands on and every row
/// after it, so one at the frontier -- where ordinary play puts all of them --
/// touches a single entry, and one `n` rows back touches the `n + 1` entries
/// from that row to the frontier.
///
/// # What a decoded log is not checked for here
///
/// The fields are private and the constructors keep `actions` rectangular, but
/// `Deserialize` writes them directly and a corrupt or hand-made capture can
/// arrive with a length that is not a whole number of rows. Nothing in this
/// type refuses that, and the reason is that this type cannot tell the two
/// cases apart: [`ticks`](Self::ticks) counts whole rows, so the entries past
/// the last one are unreachable through every accessor here and read exactly
/// like a log that does not have them. They stop being unreachable at the first
/// [`extend_to`](Self::extend_to), which hands them to the new row's first
/// seats. [`Session::check`](crate::Session::check) is what refuses them, with
/// [`Shape::Ragged`](crate::Shape::Ragged), before a capture becomes a session.
#[derive(Clone, Debug, Eq, Serialize)]
pub struct ActionLog<A> {
    /// The tick the first row belongs to.
    first: Tick,
    /// How many seats wide a row is.
    players: u16,
    /// `ticks * players` entries, row-major, ordered by tick and then by seat.
    actions: Vec<A>,
    /// One bit per entry, least significant bit first, set where somebody has
    /// confirmed a value. Bytes past the end of the entries read as zero, so a
    /// short bitset is unconfirmed rather than out of range.
    confirmed: Vec<u8>,
    /// One entry per row: how many corrections have landed on this row or an
    /// earlier one. Non-decreasing, so the count for a prefix is a single read.
    ///
    /// Not written down and not compared. It is about the relationship between
    /// *this* log and *this* machine's snapshot ring rather than about the
    /// session: two peers holding the same actions took them in different
    /// orders and at different times, a capture that is loaded has no ring to
    /// be stale against, and a log that serialized this would make two peers'
    /// identical sessions unequal.
    #[serde(skip)]
    corrections: Vec<u64>,
}

impl<A> ActionLog<A> {
    /// An empty log for a session that opens at `first` with `players` seats.
    #[must_use]
    pub const fn new(first: Tick, players: u16) -> Self {
        Self {
            first,
            players,
            actions: Vec::new(),
            confirmed: Vec::new(),
            corrections: Vec::new(),
        }
    }

    /// The tick the first row belongs to.
    #[must_use]
    pub const fn first(&self) -> Tick {
        self.first
    }

    /// How many seats wide a row is.
    #[must_use]
    pub const fn players(&self) -> u16 {
        self.players
    }

    /// How many whole rows are recorded.
    ///
    /// A log with no seats has no rows to count and answers zero however many
    /// ticks the session ran, because a row of no actions occupies no entries
    /// and there is nothing left to divide. That is a wart of the dense layout
    /// rather than a claim about the session, and it is why a `Session` whose
    /// roster is empty can only [`seek`](crate::Session::seek) to its opening
    /// tick.
    #[must_use]
    pub fn ticks(&self) -> u64 {
        if self.players == 0 {
            return 0;
        }
        u64::try_from(self.actions.len() / usize::from(self.players)).unwrap_or(u64::MAX)
    }

    /// The latest tick a state can be reached at from this log.
    ///
    /// The row at tick `T` holds the actions that carry the state at `T` to the
    /// state at `T + 1`, so `ticks` rows reach one tick further than the last
    /// row they index.
    #[must_use]
    pub fn last(&self) -> Tick {
        self.first.saturating_add(self.ticks())
    }

    /// Every seat's action for one tick, in seat order, or an empty slice for a
    /// tick this log does not cover.
    #[must_use]
    pub fn row(&self, tick: Tick) -> &[A] {
        let Some(start) = self.start_of(tick) else {
            return &[];
        };
        start
            .checked_add(usize::from(self.players))
            .and_then(|end| self.actions.get(start..end))
            .unwrap_or_default()
    }

    /// One seat's action for one tick, or [`None`] for an entry this log does
    /// not cover.
    #[must_use]
    pub fn get(&self, tick: Tick, player: PlayerId) -> Option<&A> {
        self.actions.get(self.index_of(tick, player)?)
    }

    /// Whether anybody has [`set`](Self::set) this entry.
    ///
    /// This is not "the entry differs from the default". A seat that confirmed
    /// an idle action reads `true` here and holds `A::default()` there, which is
    /// the distinction the bit exists for.
    #[must_use]
    pub fn is_confirmed(&self, tick: Tick, player: PlayerId) -> bool {
        let Some(index) = self.index_of(tick, player) else {
            return false;
        };
        let Some(byte) = self.confirmed.get(index / 8) else {
            return false;
        };
        byte & (1 << (index % 8)) != 0
    }

    /// How many corrections have landed on rows strictly before `tick`.
    ///
    /// This is what keys a snapshot to a log. The state at `tick` is a function
    /// of the opening and of the rows before `tick`, so two answers that agree
    /// mean no row that state was built from has changed since -- and a snapshot
    /// whose recorded number no longer matches is a state of a history that did
    /// not happen. [`Snapshots::keep`](crate::Snapshots::keep) records it and
    /// [`Session::seek`](crate::Session::seek) checks it.
    ///
    /// A tick at or before the log's first has no rows before it and answers
    /// zero; a tick past the last row answers the whole count, which is what a
    /// state simulated to the frontier depends on.
    #[must_use]
    pub fn generation_at(&self, tick: Tick) -> u64 {
        if tick <= self.first {
            return 0;
        }
        let rows = tick.since(self.first);
        let index = usize::try_from(rows.saturating_sub(1)).unwrap_or(usize::MAX);
        self.corrections
            .get(index)
            .or_else(|| self.corrections.last())
            .copied()
            .unwrap_or(0)
    }

    /// How many corrections this log has taken in all.
    ///
    /// The same number [`generation_at`](Self::generation_at) reports for a tick
    /// past the last row. A log nobody has contradicted answers zero, and a
    /// growing answer is how a runtime knows a rollback happened at all.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.corrections.last().copied().unwrap_or(0)
    }

    /// How many entries the log holds, whole rows or otherwise.
    ///
    /// This and [`confirmed_bytes`](Self::confirmed_bytes) are the two numbers
    /// [`Session::load`](crate::Session::load) compares to find a decoded log
    /// whose confirmed bitmap does not cover its actions.
    #[must_use]
    pub const fn entries(&self) -> usize {
        self.actions.len()
    }

    /// How many bytes the confirmed bitmap holds.
    ///
    /// A well-formed log holds exactly [`entries`](Self::entries) rounded up to
    /// a whole number of bytes. See [`Session::load`](crate::Session::load) for
    /// what a decoded log that holds some other number costs.
    #[must_use]
    pub const fn confirmed_bytes(&self) -> usize {
        self.confirmed.len()
    }

    /// Drops every row before `tick` and moves the log's first tick to it.
    ///
    /// This is what keeps a forward-only run from holding an hour of actions it
    /// will never read. It is the opposite end of the log from
    /// [`extend_to`](Self::extend_to) and it is not an inverse of anything: the
    /// rows are gone, and the states they produced can no longer be reached from
    /// this log. [`Session::forget_before`](crate::Session::forget_before) is
    /// where it is called from, because a log that has forgotten its opening
    /// rows is only a session again once the opening has moved with it.
    ///
    /// A `tick` at or before [`first`](Self::first) has no rows before it and
    /// changes nothing. A `tick` past [`last`](Self::last) drops every row and
    /// leaves an empty log positioned there, which is the same log a
    /// [`new`](Self::new) at that tick would be.
    ///
    /// For any `tick` the log covers -- which is every call a running loop
    /// makes, since a loop forgets behind the frontier it is writing at --
    /// [`last`](Self::last) does not move. That is the property that makes this
    /// safe to call from inside one: the frontier is exactly where it was, and
    /// only how far back the log reaches has changed. A `tick` past
    /// [`last`](Self::last) is the exception and moves it, because the log it
    /// leaves behind is an empty log at `tick` and an empty log's last row is
    /// where it starts. [`Session::forget_before`](crate::Session::forget_before)
    /// refuses that case with [`Forget::Beyond`](crate::Forget::Beyond) rather
    /// than passing it on, so a session cannot reach it.
    ///
    /// # What this does to a snapshot ring
    ///
    /// The corrections a retained row carries are kept, so
    /// [`generation_at`](Self::generation_at) answers what it answered before
    /// for every tick after the new first -- which is what leaves a ring's
    /// entries reachable across a forget. The tick it reopens at is the
    /// exception: the rows its state was built from are the ones that have just
    /// gone, so the count there is zero again, and an entry kept at exactly that
    /// tick under a higher count is skipped rather than trusted.
    ///
    /// That is one direction of a comparison and not both. An entry kept at that
    /// tick under a count of zero, which a *later* correction to a forgotten row
    /// had taken out of reach, is reachable again -- the log that could tell
    /// the two apart is the log this call threw away. A caller that both
    /// forgets a prefix and holds a ring wants
    /// [`Snapshots::discard_from`](crate::Snapshots::discard_from) at the tick it
    /// reopened on, for the same reason a log *replaced* wholesale wants
    /// [`Snapshots::clear`](crate::Snapshots::clear).
    pub fn forget_before(&mut self, tick: Tick) {
        if tick <= self.first {
            return;
        }
        let rows = tick.since(self.first).min(self.ticks());
        let entries = usize::try_from(rows.saturating_mul(u64::from(self.players)))
            .unwrap_or(usize::MAX)
            .min(self.actions.len());
        self.actions.drain(..entries);
        self.forget_confirmations(entries);
        let dropped = usize::try_from(rows)
            .unwrap_or(usize::MAX)
            .min(self.corrections.len());
        self.corrections.drain(..dropped);
        self.first = tick;
    }

    /// Slides the confirmation bitmap down by the `bits` entries that have just
    /// gone, and shortens it to what is left.
    ///
    /// A bitmap is a whole number of bytes and a row is however many seats wide
    /// the session is, so the entries a forget drops are almost never a whole
    /// number of bytes: a one-seat log drops one bit per row. Draining bytes
    /// alone would therefore leave every remaining entry reading somebody else's
    /// bit, which is a log that has silently exchanged "confirmed" for
    /// "nobody said" across all of its seats -- and both of those are legal
    /// values, so nothing downstream would notice.
    ///
    /// The bits above the last entry go to zero on the way out, and it is worth
    /// being exact about which of them could have been anything else. Not the
    /// shift's: the byte above the top one reads as zero, so the shift brings
    /// zeros down into the bits it vacates and a log built by the constructors
    /// here is already clean. What is not clean is a **decoded** log.
    /// `Deserialize` writes the bitmap verbatim, and
    /// [`Session::check`](crate::Session::check) compares its *length* against
    /// the entries rather than its contents -- so a corrupt or hand-made capture
    /// can arrive with bits set past its last entry, pass every check there is,
    /// and be forgotten from. Nothing reads those bits --
    /// [`is_confirmed`](Self::is_confirmed) is bounded by the entries and
    /// [`extend_to`](Self::extend_to) clears them before a new row lands on
    /// them -- but they are written down and compared, so leaving them there
    /// would make a log that forgot a prefix unequal to a log that only ever
    /// held the rows it now has. `tests/forget.rs` builds one of those captures
    /// and asserts the two are equal afterwards.
    fn forget_confirmations(&mut self, bits: usize) {
        let bytes = (bits / 8).min(self.confirmed.len());
        self.confirmed.drain(..bytes);

        let rest = bits % 8;
        if rest != 0 {
            for index in 0..self.confirmed.len() {
                let low = self.confirmed.get(index).copied().unwrap_or(0) >> rest;
                let high = self.confirmed.get(index + 1).copied().unwrap_or(0) << (8 - rest);
                if let Some(byte) = self.confirmed.get_mut(index) {
                    *byte = low | high;
                }
            }
        }

        let entries = self.actions.len();
        self.confirmed.truncate(entries.div_ceil(8));
        if !entries.is_multiple_of(8)
            && let Some(byte) = self.confirmed.last_mut()
        {
            *byte &= (1_u8 << (entries % 8)).wrapping_sub(1);
        }
    }

    /// Where `tick`'s row starts, if this log covers it.
    fn start_of(&self, tick: Tick) -> Option<usize> {
        if tick < self.first {
            return None;
        }
        let row = tick.since(self.first);
        let start = usize::try_from(row.checked_mul(u64::from(self.players))?).ok()?;
        (row < self.ticks()).then_some(start)
    }

    /// Where one entry lives, if this log covers it.
    fn index_of(&self, tick: Tick, player: PlayerId) -> Option<usize> {
        if player.0 >= self.players {
            return None;
        }
        self.start_of(tick)?.checked_add(usize::from(player.0))
    }
}
