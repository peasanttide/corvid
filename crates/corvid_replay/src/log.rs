//! One action per player per tick, dense, with the confirmed entries marked.

use alloc::vec::Vec;
use core::{
    fmt,
    hash::{Hash, Hasher},
};

use corvid_behavior::PlayerId;
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

/// The actions of a session, one per seat per tick, laid out flat.
///
/// The entry for `tick` and `player` lives at index
/// `(tick - first) * players + player`, so a row is contiguous and a lookup is
/// arithmetic rather than a search. Nothing is optional: an entry inside the
/// recorded range that nobody has [`set`](Self::set) holds `A::default()`,
/// which is what "this player did nothing" already means in `corvid_behavior`.
/// That is the whole reason the log is dense rather than sparse — a game never
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
/// have landed on rows strictly before a given tick — which is exactly the set
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
/// The looser rule — count the row at `T` as well — reads like a safe
/// over-approximation and is not one. An over-approximation would throw away
/// some entries that were still good and keep the rest; this one keeps nothing
/// at all. Ordinary forward play keeps the state at `S` and only then learns
/// what the seats did on `S`, so writing row `S` would invalidate the snapshot
/// taken at `S` moments earlier — on every tick, for every entry, in the case
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
/// after it, so one at the frontier — where ordinary play puts all of them —
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

/// The actions and where they sit, and never the generation.
///
/// Two logs are equal when they record the same session. The generation is one
/// machine's bookkeeping about its own snapshot ring, so a log written down at
/// generation forty and read back at zero is the same log — which is what
/// `tests/roundtrip.rs` asserts by comparing a saved session against the loaded
/// one.
impl<A: PartialEq> PartialEq for ActionLog<A> {
    fn eq(&self, other: &Self) -> bool {
        self.first == other.first
            && self.players == other.players
            && self.actions == other.actions
            && self.confirmed == other.confirmed
    }
}

/// The same four fields [`PartialEq`] compares, in the same order.
///
/// Hand-written rather than derived, for the reason directly above: a derived
/// [`Hash`] would absorb the generation, and two logs that record the same
/// session and compare equal would then hash apart — which is the one thing an
/// implementation of this trait is not allowed to do.
impl<A: Hash> Hash for ActionLog<A> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.first.hash(state);
        self.players.hash(state);
        self.actions.hash(state);
        self.confirmed.hash(state);
    }
}

/// The four fields a log is written down as, which is every field but the
/// generation.
#[derive(Deserialize)]
struct Wire<A> {
    /// The tick the first row belongs to.
    first: Tick,
    /// How many seats wide a row is.
    players: u16,
    /// The entries, row-major.
    actions: Vec<A>,
    /// The confirmation bitmap.
    confirmed: Vec<u8>,
}

/// Reads the four recorded fields and gives the log a generation of its own.
///
/// A decoded log has taken no corrections *on this machine*, whatever it took on
/// the one that recorded it, and there is no snapshot ring here that could be
/// stale against those. So every row starts at zero — one entry per row, because
/// a generation that was left empty would silently stop counting the corrections
/// a rollback then makes.
impl<'de, A: Deserialize<'de>> Deserialize<'de> for ActionLog<A> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Wire::<A>::deserialize(deserializer)?;
        let mut log = Self {
            first: wire.first,
            players: wire.players,
            actions: wire.actions,
            confirmed: wire.confirmed,
            corrections: Vec::new(),
        };
        // Bounded by the entries that have already been decoded, so this cannot
        // be asked for more than the actions themselves already cost.
        let rows = usize::try_from(log.ticks()).unwrap_or(usize::MAX);
        log.corrections.resize(rows, 0);
        Ok(log)
    }
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
    /// mean no row that state was built from has changed since — and a snapshot
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
    /// For any `tick` the log covers — which is every call a running loop
    /// makes, since a loop forgets behind the frontier it is writing at —
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
    /// for every tick after the new first — which is what leaves a ring's
    /// entries reachable across a forget. The tick it reopens at is the
    /// exception: the rows its state was built from are the ones that have just
    /// gone, so the count there is zero again, and an entry kept at exactly that
    /// tick under a higher count is skipped rather than trusted.
    ///
    /// That is one direction of a comparison and not both. An entry kept at that
    /// tick under a count of zero, which a *later* correction to a forgotten row
    /// had taken out of reach, is reachable again — the log that could tell
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
    /// "nobody said" across all of its seats — and both of those are legal
    /// values, so nothing downstream would notice.
    ///
    /// The bits above the last entry go to zero on the way out, and it is worth
    /// being exact about which of them could have been anything else. Not the
    /// shift's: the byte above the top one reads as zero, so the shift brings
    /// zeros down into the bits it vacates and a log built by the constructors
    /// here is already clean. What is not clean is a **decoded** log.
    /// `Deserialize` writes the bitmap verbatim, and
    /// [`Session::check`](crate::Session::check) compares its *length* against
    /// the entries rather than its contents — so a corrupt or hand-made capture
    /// can arrive with bits set past its last entry, pass every check there is,
    /// and be forgotten from. Nothing reads those bits —
    /// [`is_confirmed`](Self::is_confirmed) is bounded by the entries and
    /// [`extend_to`](Self::extend_to) clears them before a new row lands on
    /// them — but they are written down and compared, so leaving them there
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

impl<A: Clone + Default> ActionLog<A> {
    /// Grows the log so that `tick` has a row, filling anything new with
    /// `A::default()` and leaving it unconfirmed.
    ///
    /// This is separate from [`set`](Self::set), and the separation is the
    /// point. A tick number is the one thing in a session that arrives from
    /// somewhere else — a peer's packet, a save file, a slider — and a `set`
    /// that grew on demand would turn `Tick(u64::MAX)` from a remote peer into
    /// a request for sixteen exabytes of actions. Growing is therefore a
    /// decision the runtime makes explicitly, at a call site that knows how far
    /// ahead of the present it is willing to record.
    ///
    /// The room is asked for with `try_reserve`, so a request this machine
    /// cannot satisfy is [`Refused::Memory`] rather than an abort.
    ///
    /// "Leaving it unconfirmed" is a promise this has to keep rather than one
    /// the layout makes on its own. A bitmap is a whole number of bytes, so the
    /// bits above the last entry are storage nothing has asked about yet, and a
    /// decoded one can arrive with any of them set: its length is all
    /// [`Session::check`](crate::Session::check) can compare. Those bits are
    /// exactly the ones the next row lands on, so they are cleared here. A row
    /// that appeared already holding somebody else's confirmations would refuse
    /// the packets that belong to it, with
    /// [`Refused::Confirmed`](Refused::Confirmed) against an action nobody sent.
    ///
    /// # Errors
    ///
    /// [`Refused::Early`] if `tick` is before the log's first tick, and
    /// [`Refused::Memory`] if the entries the request needs could not be
    /// reserved.
    pub fn extend_to(&mut self, tick: Tick) -> Result<(), Refused> {
        if tick < self.first {
            return Err(Refused::Early {
                tick,
                first: self.first,
            });
        }
        let rows = tick.since(self.first).saturating_add(1);
        if rows <= self.ticks() {
            return Ok(());
        }
        let memory = || Refused::Memory { rows };
        let entries = usize::try_from(
            rows.checked_mul(u64::from(self.players))
                .ok_or_else(memory)?,
        )
        .map_err(|_| memory())?;

        let held = self.actions.len();
        let bytes = entries.div_ceil(8);
        let count = usize::try_from(rows).map_err(|_| memory())?;

        // All three reservations before any of the three resizes, so that a
        // request this machine cannot satisfy leaves the log exactly as it was.
        // Growing the actions first would leave rows the bitmap and the
        // generation vector do not cover if either of the later reservations
        // failed, and the row counts are precisely what `Session::check`
        // compares: a log that survived a refused `extend_to` has entries whose
        // confirmation bit `set` silently discards, so every contradicting
        // write to them is accepted and `Refused::Confirmed` never fires again.
        // The corrections branch is the likelier of the two to fail, because
        // for a one-byte action it asks for eight times the bytes the actions
        // asked for. Each `resize` below is inside the capacity just reserved
        // and so cannot allocate.
        self.actions
            .try_reserve(entries.saturating_sub(held))
            .map_err(|_| memory())?;
        self.confirmed
            .try_reserve(bytes.saturating_sub(self.confirmed.len()))
            .map_err(|_| memory())?;
        self.corrections
            .try_reserve(count.saturating_sub(self.corrections.len()))
            .map_err(|_| memory())?;

        // A row that has just appeared has taken no corrections of its own, so
        // it carries whatever every earlier row has taken between them. Read
        // before the corrections grow, because growing them would answer with
        // the value being carried in.
        let carried = self.generation();

        self.actions.resize(entries, A::default());
        self.confirmed.resize(bytes, 0);

        // Every bit from the entry count this log arrived with upwards belongs
        // to an entry that has just appeared, whatever it held before. The byte
        // that straddles the boundary keeps its lower bits and loses the rest;
        // the bytes above it are entirely new entries' and go to zero.
        if let Some(byte) = self.confirmed.get_mut(held / 8) {
            *byte &= (1_u8 << (held % 8)).wrapping_sub(1);
        }
        if let Some(above) = self.confirmed.get_mut(held / 8 + 1..) {
            above.fill(0);
        }

        self.corrections.resize(count, carried);
        Ok(())
    }
}

impl<A: Clone + Default + PartialEq> ActionLog<A> {
    /// Records one seat's action for one tick.
    ///
    /// Idempotent for a value already confirmed there, and an error for a
    /// different one. That pair is how a rollback tells a correction from a
    /// duplicate: a packet that arrives twice is `Ok(())` the second time and
    /// changes nothing, and a packet that contradicts what the session has
    /// already agreed on is refused rather than quietly rewriting history that
    /// other peers have already simulated.
    ///
    /// The tick must already have a row. See [`extend_to`](Self::extend_to) for
    /// why growing is not this function's job.
    ///
    /// # What counts as a correction
    ///
    /// A write that *changes* the stored action, and only that. An entry
    /// nobody has confirmed holds `A::default()`, so confirming a default
    /// leaves the actions exactly as they were and writing a real action over
    /// one is the correction a peer that simulated ahead has to be told about.
    /// A second write of a value already confirmed there changes nothing and is
    /// not one either. See [`generation_at`](Self::generation_at) for what the
    /// count is then used for; every snapshot of a state built from the row
    /// this touches is stale from here on.
    ///
    /// # Errors
    ///
    /// [`Refused::Seat`] if the seat is not in the log's width,
    /// [`Refused::Early`] if the tick is before the log's first,
    /// [`Refused::Beyond`] if the tick has no row yet, and
    /// [`Refused::Confirmed`] if a different action is already confirmed there.
    pub fn set(&mut self, tick: Tick, player: PlayerId, action: A) -> Result<(), Refused> {
        if player.0 >= self.players {
            return Err(Refused::Seat {
                player,
                players: self.players,
            });
        }
        if tick < self.first {
            return Err(Refused::Early {
                tick,
                first: self.first,
            });
        }
        let beyond = Refused::Beyond {
            tick,
            first: self.first,
            rows: self.ticks(),
        };
        let index = self.index_of(tick, player).ok_or(beyond)?;
        let confirmed = self.is_confirmed(tick, player);
        // A row is only writable if the bitmap has a bit for it as well as an
        // entry. Refused rather than written-and-not-confirmed, because the bit
        // is what makes the write authoritative: an entry whose confirmation is
        // silently dropped accepts every contradicting write that follows it,
        // `Refused::Confirmed` never fires there again, and `generation` climbs
        // once per duplicate packet, invalidating the whole snapshot ring each
        // time. A log can arrive here short — `corvid_wire::decode` reaches this
        // type without passing `Session::check` — and `Beyond`'s remedy, growing
        // the log, is the one that restores the bitmap too.
        if self.confirmed.len() <= index / 8 {
            return Err(beyond);
        }
        let entry = self.actions.get_mut(index).ok_or(beyond)?;

        if confirmed {
            return if *entry == action {
                Ok(())
            } else {
                Err(Refused::Confirmed { tick, player })
            };
        }

        let corrected = *entry != action;
        *entry = action;
        if let Some(byte) = self.confirmed.get_mut(index / 8) {
            *byte |= 1 << (index % 8);
        }
        if corrected {
            self.count_a_correction(tick);
        }
        Ok(())
    }

    /// Records that the row at `tick` now says something else than it did.
    ///
    /// Every row from there on carries one more correction, and every row
    /// before it carries what it carried: a state built from the earlier rows
    /// alone is untouched by this and is the whole reason a rollback does not
    /// replay from the opening.
    fn count_a_correction(&mut self, tick: Tick) {
        let from = usize::try_from(tick.since(self.first)).unwrap_or(usize::MAX);
        if let Some(rows) = self.corrections.get_mut(from..) {
            for row in rows {
                *row = row.saturating_add(1);
            }
        }
    }
}

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
