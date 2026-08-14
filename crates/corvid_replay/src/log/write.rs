//! Writing to an action log, and the rules about what may be overwritten.
//!
//! Apart from [`the log itself`](super) because this is where the log stops being a
//! container and starts being a protocol: an entry nobody has set may be
//! written freely, a confirmed entry may not be written at all, and a
//! *correction* -- a confirmed entry that disagrees with what was predicted --
//! bumps the generation so that everything derived from the rows after it knows
//! to be recomputed. None of that is visible from the reading side.

use corvid_behavior::PlayerId;
use corvid_time::Tick;

use super::{ActionLog, Refused};

impl<A: Clone + Default> ActionLog<A> {
    /// Grows the log so that `tick` has a row, filling anything new with
    /// `A::default()` and leaving it unconfirmed.
    ///
    /// This is separate from [`set`](Self::set), and the separation is the
    /// point. A tick number is the one thing in a session that arrives from
    /// somewhere else -- a peer's packet, a save file, a slider -- and a `set`
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
        // time. A log can arrive here short -- `corvid_wire::decode` reaches this
        // type without passing `Session::check` -- and `Beyond`'s remedy, growing
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
