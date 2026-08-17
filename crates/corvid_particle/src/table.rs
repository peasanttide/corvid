//! The emitters a system holds, and the counting that lets a particle outlive
//! one.

use alloc::vec::Vec;

use crate::{Emitter, EmitterId, ParticleError};

/// One emitter and what the table has to remember about it.
#[derive(Clone, Debug)]
pub(crate) struct Slot {
    /// The description itself.
    pub(crate) emitter: Emitter,
    /// The fraction of a particle its rate has accumulated but not yet emitted.
    pub(crate) accumulator: f32,
    /// Which occupant of this slot the emitter is, so a stale [`EmitterId`] can
    /// be told from a live one.
    generation: u32,
    /// How many live particles read this slot, and so the reason a removed slot
    /// is not freed at once.
    users: u32,
    /// Whether [`Table::remove`] has been called on it.
    retired: bool,
}

/// The emitters, by index, with a free list and a hold count on each.
///
/// The whole of this type exists for one rule: **a particle outlives the
/// emitter that made it**. A particle keeps a slot index rather than a copy of
/// its emitter, which is what keeps it at fifty-six bytes rather than a hundred
/// and eighty-four, and that only works if the slot is still there to be read.
/// So
/// [`remove`](Self::remove) retires a slot instead of freeing it, every live
/// particle holds it, and the free list gets it back when the last hold goes.
///
/// A plume of smoke should not vanish when the wall it came off finishes
/// burning, and that is the same rule seen from the other end.
#[derive(Clone, Debug, Default)]
pub(crate) struct Table {
    /// The slots. Never shrinks; freed ones are reused.
    slots: Vec<Slot>,
    /// Indices with no emitter and no particle left in them.
    free: Vec<usize>,
}

impl Table {
    /// Adds an emitter and answers the handle to it.
    pub(crate) fn add(&mut self, emitter: Emitter) -> EmitterId {
        let fresh = Slot {
            emitter,
            accumulator: 0.0,
            generation: 0,
            users: 0,
            retired: false,
        };
        if let Some(index) = self.free.pop()
            && let Some(existing) = self.slots.get_mut(index)
        {
            // The generation moves on so that every id handed out for this slot
            // before now is refused from here on.
            let generation = existing.generation.wrapping_add(1);
            *existing = Slot {
                generation,
                ..fresh
            };
            return EmitterId { index, generation };
        }
        let index = self.slots.len();
        self.slots.push(fresh);
        EmitterId {
            index,
            generation: 0,
        }
    }

    /// Retires an emitter, freeing its slot if nothing is holding it.
    ///
    /// # Errors
    ///
    /// [`ParticleError::UnknownEmitter`] if the id names no live emitter.
    pub(crate) fn remove(&mut self, id: EmitterId) -> Result<(), ParticleError> {
        let index = self.resolve(id)?;
        if let Some(slot) = self.slots.get_mut(index) {
            slot.retired = true;
            if slot.users == 0 {
                self.free.push(index);
            }
        }
        Ok(())
    }

    /// The slot an id names, if it still names one.
    ///
    /// # Errors
    ///
    /// [`ParticleError::UnknownEmitter`] if the id names no live emitter.
    pub(crate) fn resolve(&self, id: EmitterId) -> Result<usize, ParticleError> {
        match self.slots.get(id.index) {
            Some(slot) if !slot.retired && slot.generation == id.generation => Ok(id.index),
            _ => Err(ParticleError::UnknownEmitter(id)),
        }
    }

    /// The emitter behind an id.
    ///
    /// # Errors
    ///
    /// [`ParticleError::UnknownEmitter`] if the id names no live emitter.
    pub(crate) fn get(&self, id: EmitterId) -> Result<&Emitter, ParticleError> {
        let index = self.resolve(id)?;
        self.slots
            .get(index)
            .map(|slot| &slot.emitter)
            .ok_or(ParticleError::UnknownEmitter(id))
    }

    /// The emitter behind an id, to change.
    ///
    /// # Errors
    ///
    /// [`ParticleError::UnknownEmitter`] if the id names no live emitter.
    pub(crate) fn get_mut(&mut self, id: EmitterId) -> Result<&mut Emitter, ParticleError> {
        let index = self.resolve(id)?;
        self.slots
            .get_mut(index)
            .map(|slot| &mut slot.emitter)
            .ok_or(ParticleError::UnknownEmitter(id))
    }

    /// A slot by index, live or retired. Retired ones are still read, because
    /// their particles are still in the air.
    pub(crate) fn slot(&self, index: usize) -> Option<&Slot> {
        self.slots.get(index)
    }

    /// A slot by index, to change.
    pub(crate) fn slot_mut(&mut self, index: usize) -> Option<&mut Slot> {
        self.slots.get_mut(index)
    }

    /// How many slots there are, which is what an emission pass walks.
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    /// Records that one more particle is reading a slot.
    pub(crate) fn attach(&mut self, index: usize) {
        if let Some(slot) = self.slots.get_mut(index) {
            slot.users += 1;
        }
    }

    /// Records that one fewer is, and frees the slot if that was the last hold
    /// on a retired one.
    pub(crate) fn release(&mut self, index: usize) {
        if let Some(slot) = self.slots.get_mut(index) {
            slot.users = slot.users.saturating_sub(1);
            if slot.retired && slot.users == 0 {
                self.free.push(index);
            }
        }
    }
}

impl Slot {
    /// Whether this slot's rate should be making particles.
    ///
    /// A retired emitter emits nothing however its rate is set: what is left of
    /// it is a description its own particles are still reading.
    pub(crate) fn emitting(&self) -> bool {
        !self.retired && self.emitter.rate > 0.0
    }
}
