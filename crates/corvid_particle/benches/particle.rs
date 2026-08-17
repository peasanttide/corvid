//! What ten thousand particles cost.
//!
//! ```sh
//! cargo bench -p corvid_particle
//! ```
//!
//! The budget this exists to hold is a frame's: at sixty frames a second a
//! frame is 16.6 ms, and a particle system that is not the game has no business
//! taking a tenth of it. **The stated budget is one millisecond to step ten
//! thousand particles**, and where it was written it takes about 150 us of
//! that, which is one percent of a frame for a district on fire.
//!
//! Three rows rather than one because they answer different questions.
//! Stepping is the cost of a fire that is already burning; writing is the cost
//! of drawing it, at about 280 us for the same ten thousand, and a game that
//! sorts its instances pays that one again on top; bursting is the cost of
//! something happening, and it is the expensive one at about 2.3 ms for ten
//! thousand at once.
//!
//! That last figure is worth knowing rather than hiding. A spawn draws a
//! direction, and a direction is a square root, a sine and a cosine -- and
//! `core` has none of those, so they are `corvid_float`'s software ones rather
//! than the hardware's. Two hundred nanoseconds a particle is nothing against a
//! blast of the five hundred an explosion actually wants, which is 120 us, and
//! it is a visible hitch at ten thousand. A caller who genuinely wants ten
//! thousand at once should spread them over several steps, and this row is how
//! they would know that.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

use corvid_glm::Vec3;
use corvid_particle::{Emitter, Instance, Range, Shape, System};

/// The load. Ten thousand is a district on fire rather than a campfire.
const PARTICLES: u32 = 10_000;

/// A sixtieth of a second, which is the step a display frame asks for.
const DT: f32 = 1.0 / 60.0;

/// An emitter of embers: gravity, drag, a spin and a growth, so that every part
/// of the step is exercised rather than the cheapest path through it.
fn embers() -> Emitter {
    let mut emitter = Emitter::new(Vec3::zeros(), Shape::Sphere { radius: 2.0 });
    emitter.speed = Range::new(1.0, 12.0);
    // Long enough that nothing dies during a measurement, so that what is timed
    // is the same work every iteration.
    emitter.lifetime = Range::exactly(1.0e6);
    emitter.size = Range::new(0.05, 0.2);
    emitter.size_end = 2.0;
    emitter.spin = Range::new(-8.0, 8.0);
    emitter.gravity = Vec3::new(0.0, 0.0, -9.81);
    emitter.drag = 2.0;
    emitter
}

/// A full system, ready to be stepped.
fn filled() -> System {
    let mut system = System::new(PARTICLES as usize, 17_890_428);
    let id = system.add(embers());
    let _ = system.burst(id, PARTICLES);
    system
}

/// Stepping a full pool, which is what a frame does whether or not anything is
/// happening.
fn stepping(c: &mut Criterion) {
    let mut system = filled();
    c.bench_function("step/10000", |b| {
        b.iter(|| system.step(black_box(DT)));
    });
}

/// Emitting a full pool at once, which is what a blast does.
fn bursting(c: &mut Criterion) {
    c.bench_function("burst/10000", |b| {
        b.iter_batched_ref(
            || {
                let mut system = System::new(PARTICLES as usize, 17_890_428);
                let id = system.add(embers());
                (system, id)
            },
            |(system, id)| {
                let _ = system.burst(*id, black_box(PARTICLES));
            },
            BatchSize::SmallInput,
        );
    });
}

/// Writing the pool out as instances, into a buffer the caller keeps.
///
/// The `Vec` is reused between iterations because a game reuses its own: an
/// allocation a frame would be the thing measured otherwise.
fn writing(c: &mut Criterion) {
    let system = filled();
    let mut buffer: Vec<Instance> = Vec::with_capacity(PARTICLES as usize);
    c.bench_function("write/10000", |b| {
        b.iter(|| {
            buffer.clear();
            buffer.extend(system.instances());
            black_box(buffer.len());
        });
    });
}

criterion_group!(benches, stepping, bursting, writing);
criterion_main!(benches);
