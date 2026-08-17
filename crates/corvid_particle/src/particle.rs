//! One live particle: the state a step reads and writes.

use corvid_glm::Vec3;

use crate::{Emitter, Instance};

/// A live particle, fifty-six bytes of it.
///
/// What is here is what changes or was drawn at birth; what is not here is
/// everything shared with the rest of the emitter's particles -- the ramp, the
/// growth, the gravity, the drag -- which is read back through
/// [`slot`](Self::slot) each step. That is the difference between fifty-six
/// bytes a particle and the hundred and eighty-four an [`Emitter`] weighs, and
/// at the ten thousand the benchmark steps it is the difference between half a
/// megabyte and nearly two.
///
/// It costs one indirection per particle per step and one rule: an emitter's
/// slot outlives the emitter, because a particle in the air still has to be
/// able to read it. `System::remove` retires a slot rather than freeing it, and
/// the free list gets it back when the last particle naming it has died.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Particle {
    /// Where it is, in the system's frame.
    pub position: Vec3,
    /// How fast it is going, in metres a second.
    pub velocity: Vec3,
    /// How long it has been alive, in seconds.
    pub age: f32,
    /// How long it gets, in seconds.
    pub lifetime: f32,
    /// How big it was at birth, before the growth over its life.
    pub size: f32,
    /// Radians a second it turns at.
    pub spin: f32,
    /// Which way up it was born.
    pub phase: f32,
    /// The fraction of a trail particle it owes, carried between steps so that
    /// a trail rate below one a step still trails.
    pub debt: f32,
    /// Which emitter slot made it. An index rather than an
    /// [`EmitterId`](crate::EmitterId): the generation cannot change under a
    /// particle, because the slot is not freed while one is looking at it.
    pub slot: usize,
}

impl Particle {
    /// How far through its life it is, from zero to one.
    ///
    /// A lifetime of zero reads as one -- a particle with no life left is at
    /// the end of it -- rather than as the infinity the division would give.
    #[inline]
    pub(crate) fn fraction(self) -> f32 {
        if self.lifetime > 0.0 {
            corvid_float::clamp(self.age / self.lifetime, 0.0, 1.0)
        } else {
            1.0
        }
    }

    /// Whether it has outlived its lifetime.
    #[inline]
    pub(crate) fn is_dead(self) -> bool {
        self.age >= self.lifetime
    }

    /// What a renderer is handed for it.
    pub(crate) fn instance(self, emitter: &Emitter) -> Instance {
        let fraction = self.fraction();
        Instance {
            position: [self.position.x, self.position.y, self.position.z],
            // Linear in the fraction rather than in the area: what the eye
            // reads on a smoke billboard is the width, so the width is what
            // moves evenly.
            size: self.size * (1.0 + (emitter.size_end - 1.0) * fraction),
            color: emitter.color.sample(fraction).to_f32_array(),
            rotation: self.phase + self.spin * self.age,
            age: fraction,
        }
    }
}
