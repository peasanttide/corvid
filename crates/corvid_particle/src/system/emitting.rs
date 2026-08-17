//! Where a particle comes from: the rates, the trails, the bursts, and the
//! budget that refuses one.
//!
//! A child of [`system`](super) rather than a neighbour of it, because these
//! reach into a [`System`]'s own pool and its own generator, and widening those
//! to the crate so that a sibling file could see them would widen them to every
//! file in the crate. The seam is real either way: this is how a particle comes
//! into being, and the parent is what happens to it afterwards.

use alloc::vec::Vec;
use core::mem;

use corvid_float::consts::TAU;
use corvid_glm::Vec3;

use crate::particle::Particle;
use crate::{EmitterId, ParticleError, System, Trail};

/// The most particles one emitter is allowed to produce in a single step.
///
/// A ceiling on the rate rather than a budget: a rate and a step whose product
/// is larger than this describe more particles than any pool holds, so the
/// alternative to stopping is a loop that spawns and immediately evicts for as
/// long as the numbers say. Reaching it does not count as a drop, because
/// nothing was born to be dropped.
const PER_STEP: f32 = 4096.0;

impl System {
    /// Emits `count` particles from an emitter at once, whatever its rate is.
    ///
    /// The whole count is emitted: a burst of two hundred is two hundred spawns
    /// and two hundred draws from the seeded stream, even where the pool has
    /// room for fewer. What a full pool does to them is
    /// [`dropped`](Self::dropped)'s business rather than this one's, so that a
    /// burst is the same sequence of particles whatever else is on screen.
    ///
    /// # Errors
    ///
    /// [`ParticleError::UnknownEmitter`] if the id names no live emitter.
    pub fn burst(&mut self, id: EmitterId, count: u32) -> Result<(), ParticleError> {
        let index = self.table.resolve(id)?;
        let at = match self.table.slot(index) {
            Some(slot) => slot.emitter.at,
            None => return Err(ParticleError::UnknownEmitter(id)),
        };
        for _ in 0..count {
            self.spawn(index, at);
        }
        Ok(())
    }

    /// Notes the trail particles one particle owes, spread along the segment it
    /// has just travelled.
    ///
    /// The spread is why a trail looks like a line rather than a string of
    /// beads: a fragment doing thirty metres a second crosses half a metre in a
    /// sixtieth of a second, and thirty trail particles a second all born where
    /// it happens to be at the end of the step would be that half metre apart.
    pub(super) fn owe_trail(
        particle: &mut Particle,
        trail: Trail,
        from: Vec3,
        dt: f32,
        trails: &mut Vec<(EmitterId, Vec3)>,
    ) {
        if !trail.rate.is_finite() || trail.rate <= 0.0 {
            return;
        }
        particle.debt += trail.rate * dt;
        let count = corvid_float::clamp(corvid_float::floor(particle.debt), 0.0, PER_STEP);
        particle.debt -= count;
        // Counted in floats rather than in an integer so that there is no cast
        // between the two: every value here is a whole number well under the
        // twenty-four bits an `f32` holds exactly.
        let mut nth = 0.5;
        while nth < count {
            let along = nth / count;
            trails.push((trail.emitter, from + (particle.position - from) * along));
            nth += 1.0;
        }
    }

    /// Emits everything the trails asked for during the motion pass.
    pub(super) fn emit_trails(&mut self) {
        let requests = mem::take(&mut self.trails);
        for &(id, at) in &requests {
            if let Ok(index) = self.table.resolve(id) {
                self.spawn(index, at);
            }
        }
        // Back into place, emptied, so the next step reuses the allocation.
        self.trails = requests;
        self.trails.clear();
    }

    /// Emits what every emitter's rate has earned over `dt`.
    pub(super) fn emit_rates(&mut self, dt: f32) {
        for index in 0..self.table.len() {
            let (at, count) = match self.table.slot_mut(index) {
                Some(slot) if slot.emitting() => {
                    slot.accumulator += slot.emitter.rate * dt;
                    let count =
                        corvid_float::clamp(corvid_float::floor(slot.accumulator), 0.0, PER_STEP);
                    slot.accumulator -= count;
                    (slot.emitter.at, count)
                }
                _ => continue,
            };
            let mut nth = 0.0;
            while nth < count {
                self.spawn(index, at);
                nth += 1.0;
            }
        }
    }

    /// Makes one particle from the emitter in `index`, born at `at`.
    fn spawn(&mut self, index: usize, at: Vec3) {
        if !self.make_room() {
            return;
        }
        let Some(slot) = self.table.slot(index) else {
            return;
        };
        let (shape, speed, lifetime, size, spin) = (
            slot.emitter.shape,
            slot.emitter.speed,
            slot.emitter.lifetime,
            slot.emitter.size,
            slot.emitter.spin,
        );
        let (offset, direction) = shape.sample(&mut self.rng);
        let particle = Particle {
            position: at + offset,
            velocity: direction * speed.sample(&mut self.rng),
            age: 0.0,
            lifetime: lifetime.sample(&mut self.rng),
            size: size.sample(&mut self.rng),
            spin: spin.sample(&mut self.rng),
            phase: self.rng.range(0.0, TAU),
            debt: 0.0,
            slot: index,
        };
        self.table.attach(index);
        self.live.push_back(particle);
    }

    /// Makes room for one more particle, and says whether there is any.
    ///
    /// **The oldest dies.** Not the newest, which would be cheaper still: the
    /// newest is the burst that just went off, the thing the player is looking
    /// at and the reason the pool overflowed, and refusing it makes a full
    /// system stop responding to what is happening while it goes on drawing
    /// what already has. The oldest is the most faded -- a ramp ends at
    /// nothing, which is what makes the loss cheap -- and losing it shortens
    /// the tail of the effect rather than removing its head.
    fn make_room(&mut self) -> bool {
        if self.capacity == 0 {
            self.dropped += 1;
            return false;
        }
        while self.live.len() >= self.capacity {
            match self.live.pop_front() {
                Some(oldest) => {
                    self.dropped += 1;
                    self.table.release(oldest.slot);
                }
                None => break,
            }
        }
        true
    }
}
