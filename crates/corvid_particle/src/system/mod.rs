//! The pool: emitters in, instances out, and a budget in between.
//!
//! This half is what happens to a particle that exists: the motion, the death
//! and the reading. [`emitting`] is the other half, where one comes from.

mod emitting;

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::mem;

use corvid_glm::Vec3;

use crate::motion::advance;
use crate::particle::Particle;
use crate::table::Table;
use crate::{Emitter, EmitterId, Instance, ParticleError, Rng};

/// A pool of particles, the emitters that fill it, and the seed that makes both
/// repeat.
///
/// A system owns everything: [`add`](Self::add) an [`Emitter`] to get an
/// [`EmitterId`], [`burst`](Self::burst) from it or leave its
/// [`rate`](Emitter::rate) to do the work, [`step`](Self::step) it by a
/// duration, and read [`instances`](Self::instances) for what to draw. Nothing
/// here knows what a device is and nothing here issues a draw call, which is
/// what lets the whole crate run in a test with no screen attached.
///
/// **Positions are in the system's own frame**, in metres. Which frame that is
/// belongs to the caller: an `f32` has twenty-four bits of mantissa and a
/// world-scale game is millions of metres across, so the frame that works is
/// one near the effect -- the burning building's, the shell's -- with the
/// fixed-point world position kept outside and added back when the instances
/// are written. That is the arrangement `corvid_mesh` uses for the same reason.
///
/// **Nothing here sorts.** The instances come out oldest first, which is the
/// order they were born in and not the order back-to-front alpha blending
/// needs. A caller drawing with a depth test and additive blending -- embers,
/// flame -- does not care; a caller drawing soft smoke with straight alpha has
/// to sort by view depth itself, because this crate does not know where the
/// camera is and is not going to guess.
///
/// ```
/// use corvid_glm::Vec3;
/// use corvid_particle::{Emitter, Shape, System};
///
/// let mut fire = System::new(4096, 17_890_428);
/// let ember = fire.add(Emitter::new(Vec3::zeros(), Shape::Point));
/// fire.burst(ember, 100)?;
/// assert_eq!(fire.len(), 100);
///
/// // A tenth of a second later they are all still there, having moved.
/// fire.step(0.1);
/// assert_eq!(fire.instances().count(), 100);
///
/// // And after their second of life, none of them are.
/// fire.step(1.0);
/// assert!(fire.is_empty());
/// # Ok::<(), corvid_particle::ParticleError>(())
/// ```
#[derive(Clone, Debug)]
pub struct System {
    /// The emitters, and the holds live particles have on them.
    table: Table,
    /// The live particles, oldest at the front.
    ///
    /// The order is an invariant and it is what makes the budget cheap. Every
    /// particle ages by the same `dt`, so ageing preserves it; a birth is
    /// always the youngest, so pushing at the back preserves it; and a death
    /// removes without reordering. The oldest is therefore always
    /// [`VecDeque::pop_front`], which is what the drop policy needs and what a
    /// scan for the maximum age would have cost a pass over the pool for.
    live: VecDeque<Particle>,
    /// Where trail particles want to be born, filled during a step and drained
    /// at the end of it. A field rather than a local so that a frame does not
    /// allocate.
    trails: Vec<(EmitterId, Vec3)>,
    /// The most particles that may be alive at once.
    capacity: usize,
    /// The one source of randomness.
    rng: Rng,
    /// How many particles the budget has refused.
    dropped: u64,
}

impl System {
    /// A system holding at most `capacity` particles, seeded with `seed`.
    ///
    /// The pool is allocated here rather than as it fills, because a frame that
    /// allocates is a frame that stutters and the whole point of a cap is to
    /// know the cost up front. A capacity of zero is legal and is a way to
    /// switch an effect off without unwiring it: every particle is refused and
    /// [`dropped`](Self::dropped) counts them.
    ///
    /// The seed is the caller's, and it is the only thing that decides what the
    /// particles do. Nothing in this crate reads a clock.
    #[must_use]
    pub fn new(capacity: usize, seed: u64) -> Self {
        Self {
            table: Table::default(),
            live: VecDeque::with_capacity(capacity),
            trails: Vec::new(),
            capacity,
            rng: Rng::new(seed),
            dropped: 0,
        }
    }

    /// Adds an emitter and answers the handle to it.
    #[must_use = "the emitter cannot be reached again without its id"]
    pub fn add(&mut self, emitter: Emitter) -> EmitterId {
        self.table.add(emitter)
    }

    /// Stops an emitter making particles and forgets it.
    ///
    /// The particles it has already made live out their lives, for the reason
    /// the emitter table gives: a plume of smoke should not vanish when the
    /// wall it came off finishes burning. The slot behind it is kept until the
    /// last of them has died, and only then is the id it was reached by allowed
    /// to name something else.
    ///
    /// # Errors
    ///
    /// [`ParticleError::UnknownEmitter`] if the id names no live emitter.
    pub fn remove(&mut self, id: EmitterId) -> Result<(), ParticleError> {
        self.table.remove(id)
    }

    /// The emitter behind an id.
    ///
    /// # Errors
    ///
    /// [`ParticleError::UnknownEmitter`] if the id names no live emitter.
    pub fn get(&self, id: EmitterId) -> Result<&Emitter, ParticleError> {
        self.table.get(id)
    }

    /// The emitter behind an id, to change.
    ///
    /// Changing one changes the particles it has already made as well as the
    /// ones it has not, for everything it does not draw at birth -- the colour,
    /// the growth, the gravity, the drag. [`Emitter`] says which is which.
    ///
    /// # Errors
    ///
    /// [`ParticleError::UnknownEmitter`] if the id names no live emitter.
    pub fn get_mut(&mut self, id: EmitterId) -> Result<&mut Emitter, ParticleError> {
        self.table.get_mut(id)
    }

    /// Ages every particle by `dt` seconds, buries the ones that are done, and
    /// emits what the rates and the trails have earned.
    ///
    /// A `dt` that is not a positive finite number does nothing at all. A
    /// display that has been paused, a first frame with no previous timestamp
    /// and a clock that has gone backwards all arrive as one of those, and none
    /// of them is a reason to move a particle.
    ///
    /// Emission happens after the motion, so a particle born in a step is not
    /// moved until the next one. What that costs is a stationary emitter's
    /// stream being beaded at the step rate rather than smeared along it, which
    /// is invisible while the emitter is still and is why a trail -- whose
    /// emitter is a particle, and whose motion this crate therefore knows -- is
    /// spread along the segment it travelled instead.
    pub fn step(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.advance_all(dt);
        self.emit_trails();
        self.emit_rates(dt);
    }

    /// Moves every particle, buries the dead, and notes where trails go.
    fn advance_all(&mut self, dt: f32) {
        let mut live = mem::take(&mut self.live);
        let mut trails = mem::take(&mut self.trails);
        let table = &mut self.table;
        live.retain_mut(|particle| {
            let Some(slot) = table.slot(particle.slot) else {
                return false;
            };
            // Read out of the slot rather than copied out of it: an `Emitter`
            // is a couple of hundred bytes and a step needs three fields.
            let (gravity, drag, trail) =
                (slot.emitter.gravity, slot.emitter.drag, slot.emitter.trail);
            let from = particle.position;
            particle.age += dt;
            let (position, velocity) =
                advance(particle.position, particle.velocity, gravity, drag, dt);
            particle.position = position;
            particle.velocity = velocity;
            if let Some(trail) = trail {
                Self::owe_trail(particle, trail, from, dt, &mut trails);
            }
            if particle.is_dead() {
                table.release(particle.slot);
                false
            } else {
                true
            }
        });
        self.live = live;
        self.trails = trails;
    }

    /// What to draw, oldest particle first.
    ///
    /// Borrowed rather than collected, because the caller owns the buffer this
    /// ends up in: `bytemuck::cast_slice` over a `Vec<Instance>` the caller
    /// keeps between frames is the whole of the road from here to a device.
    pub fn instances(&self) -> impl Iterator<Item = Instance> + '_ {
        self.live.iter().filter_map(|particle| {
            Some(particle.instance(&self.table.slot(particle.slot)?.emitter))
        })
    }

    /// How many particles are alive.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// Whether there is nothing to draw.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// The most particles that may be alive at once.
    #[must_use]
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// How many particles the budget has refused since this system was made:
    /// evicted to make room for a newer one, or never born because there was
    /// no room to make.
    ///
    /// A counter rather than an event, because the question it answers is asked
    /// by a test and a frame graph rather than by a person reading a log: a
    /// number that climbs while the district burns is the signal that the cap
    /// in [`new`](Self::new) is the wrong size.
    #[must_use]
    #[inline]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Kills every particle at once, leaving the emitters alone.
    ///
    /// What a camera cut wants: the smoke from the last shot has nothing to do
    /// with this one, and it should not drift across the join.
    pub fn clear(&mut self) {
        while let Some(particle) = self.live.pop_front() {
            self.table.release(particle.slot);
        }
    }
}
