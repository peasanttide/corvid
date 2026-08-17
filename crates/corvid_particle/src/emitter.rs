//! The description a system steps: what to make, where, and how it behaves.

use corvid_glm::Vec3;

use crate::{ColorRamp, Range, Shape};

/// A handle to an emitter inside a [`System`](crate::System).
///
/// An index and the generation of the slot it names, so an id outlives the
/// emitter it was made for only as a detectable error: reading through one
/// after [`System::remove`](crate::System::remove) answers
/// [`ParticleError::UnknownEmitter`](crate::ParticleError::UnknownEmitter) even
/// when a later [`System::add`](crate::System::add) has taken the slot over.
/// The generation is four bytes to make a stale handle say so, which is
/// cheaper than the hour spent finding out why the smoke is coming out of the
/// wrong wall.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EmitterId {
    /// Which slot.
    pub(crate) index: usize,
    /// Which occupant of it.
    pub(crate) generation: u32,
}

/// A particle emitted by every live particle of the emitter that carries it.
///
/// This is what makes shrapnel trail: the shrapnel is one emitter's burst, and
/// each fragment of it is itself an emitter of the smoke that marks where it
/// went. The trail's own [`Emitter::at`] is ignored while it is trailing --
/// what a trail particle is born from is the fragment's position -- and
/// everything else about the trail, its shape, its ramp and its drag, is the
/// trail emitter's own.
///
/// A trail emitter is an ordinary emitter and can be burst from, stepped and
/// removed like any other. Removing it stops the trailing; the fragments carry
/// on.
///
/// A parent is older than everything it trails, so a pool that overflows takes
/// the parent first and the trail it already laid down outlives it. That is the
/// drop policy in [`System::new`](crate::System::new) working as written rather
/// than a special case, and it is the right way round: what is left on screen
/// is the mark the fragment made, which is the part that was worth drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trail {
    /// The emitter a trail particle is made by.
    pub emitter: EmitterId,
    /// How many trail particles a second each live particle leaves.
    ///
    /// Per particle, so a hundred fragments trailing at thirty a second are
    /// three thousand particles a second between them, and the budget in
    /// [`System::new`](crate::System::new) is what stops that being a promise
    /// the frame cannot keep.
    pub rate: f32,
}

/// A description of particles: where they start, how they move, what they look
/// like as they age.
///
/// An emitter is data and holds no particles. What holds particles is the
/// [`System`](crate::System) it was added to, and a particle that has been born
/// keeps nothing but a reference to the emitter it came from -- so moving
/// [`at`](Self::at) moves where the next one appears and not where the last
/// thousand are, and widening [`color`](Self::color) recolours the ones already
/// in the air. Both are what a burning wall wants: the fire dims, and its smoke
/// dims with it.
///
/// The fields are public because an emitter is a record rather than an
/// invariant: there is no combination of them this crate rejects, and a
/// [`lifetime`](Self::lifetime) of zero, a negative [`drag`](Self::drag) or a
/// [`Range`] whose ends are crossed all have a defined meaning rather than an
/// error.
///
/// ```
/// use corvid_glm::Vec3;
/// use corvid_particle::{Emitter, Range, Shape};
///
/// // Smoke off a burning shutter: a slow cone straight up, four seconds of it,
/// // swelling as it rises and dragged toward the wind's own speed.
/// let mut smoke = Emitter::new(
///     Vec3::new(2.0, 0.0, 3.0),
///     Shape::Cone { axis: Vec3::new(0.0, 0.0, 1.0), spread: 0.35 },
/// );
/// smoke.rate = 20.0;
/// smoke.speed = Range::new(0.8, 1.6);
/// smoke.lifetime = Range::new(3.0, 5.0);
/// smoke.size = Range::new(0.4, 0.7);
/// smoke.size_end = 4.0;
/// smoke.drag = 0.6;
/// assert_eq!(smoke.at.z, 3.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Emitter {
    /// Where it is, in the system's own frame.
    pub at: Vec3,
    /// Where around [`at`](Self::at) a particle starts and which way it leaves.
    pub shape: Shape,
    /// Particles a second, continuously, whenever the system is stepped.
    ///
    /// Zero for an emitter that only ever bursts, which is what a blast is.
    /// Fractions accumulate rather than rounding, so a rate of one every ten
    /// seconds emits one every ten seconds rather than never.
    pub rate: f32,
    /// How fast a particle leaves, along the direction its shape chose.
    pub speed: Range,
    /// How long it lives, in seconds.
    pub lifetime: Range,
    /// How big it is at birth, in metres. What the number means is the
    /// renderer's business; it reaches it untouched as [`Instance::size`].
    ///
    /// [`Instance::size`]: crate::Instance::size
    pub size: Range,
    /// What that size is multiplied by at death, interpolated over the life.
    ///
    /// Four for smoke, which swells; zero for an ember, which does not shrink
    /// so much as stop being there.
    pub size_end: f32,
    /// Radians a second the particle turns at, drawn at birth and constant
    /// after it. A range that straddles zero is what stops a sheet of smoke
    /// from turning as one thing.
    pub spin: Range,
    /// What colour it is over its life.
    pub color: ColorRamp,
    /// The acceleration on it, in metres a second squared.
    ///
    /// Not the world's gravity but this emitter's: smoke sets a small positive
    /// one because it rises, an ember sets the real one because it falls, and a
    /// shockwave ring sets zero because it stays on the ground.
    pub gravity: Vec3,
    /// How fast the velocity decays toward its terminal value, per second.
    ///
    /// Zero is a vacuum, where a particle keeps whatever speed it was born
    /// with. Positive is air: the velocity approaches
    /// `gravity / drag`, so an ember with the real gravity and a drag of two
    /// settles at five metres a second downward rather than accelerating for
    /// its whole life.
    pub drag: f32,
    /// A second emitter that runs from each of this one's live particles.
    pub trail: Option<Trail>,
}

impl Emitter {
    /// An emitter at a place, with a shape, and defaults for the rest: no
    /// continuous rate, one metre a second, one second of life, ten centimetres
    /// across, no growth, no spin, white throughout, no gravity, no drag and no
    /// trail.
    ///
    /// The defaults are visible rather than pretty. An emitter built this way
    /// and burst from draws something -- white specks that fly straight and
    /// vanish after a second -- which is what a caller wiring one up needs to
    /// see before they start setting fields.
    #[must_use]
    pub fn new(at: Vec3, shape: Shape) -> Self {
        Self {
            at,
            shape,
            rate: 0.0,
            speed: Range::exactly(1.0),
            lifetime: Range::exactly(1.0),
            size: Range::exactly(0.1),
            size_end: 1.0,
            spin: Range::exactly(0.0),
            color: ColorRamp::default(),
            gravity: Vec3::zeros(),
            drag: 0.0,
            trail: None,
        }
    }
}
