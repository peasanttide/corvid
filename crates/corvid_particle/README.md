# `corvid_particle`

Smoke, embers and shrapnel, as instances a renderer can draw and nothing else.

```rust
use corvid_color::LinearRgba;
use corvid_glm::Vec3;
use corvid_particle::{ColorRamp, Emitter, Range, Shape, System};

// A powder barrel goes off in a courtyard: a fireball, and a shockwave ring
// that stays on the ground.
let mut blast = System::new(4096, 17_890_428);

let mut core = Emitter::new(Vec3::zeros(), Shape::Sphere { radius: 0.5 });
core.speed = Range::new(6.0, 14.0);
core.lifetime = Range::new(0.4, 0.9);
core.size = Range::new(0.3, 0.8);
core.size_end = 3.0;
core.drag = 3.0;
core.color = ColorRamp::fade(LinearRgba::WHITE, LinearRgba::TRANSPARENT);
let core = blast.add(core);

let mut ring = Emitter::new(Vec3::zeros(), Shape::Ring {
    normal: Vec3::new(0.0, 0.0, 1.0),
    radius: 1.0,
});
ring.speed = Range::exactly(20.0);
ring.lifetime = Range::exactly(0.5);
let ring = blast.add(ring);

blast.burst(core, 200)?;
blast.burst(ring, 64)?;
assert_eq!(blast.len(), 264);

// A frame later there are 264 instances to write into a buffer, and after the
// longest lifetime in the pair there are none.
blast.step(1.0 / 60.0);
assert_eq!(blast.instances().count(), 264);
blast.step(1.0);
assert!(blast.is_empty());
# Ok::<(), corvid_particle::ParticleError>(())
```

`no_std` plus `alloc`, it builds for `thumbv7em-none-eabi`, and it names no
graphics library at all. A [`System`] holds [`Emitter`]s and particles;
[`System::step`] ages them, moves them and emits more; [`System::instances`]
answers what is on screen as [`Instance`]s, which are forty bytes of `Pod` that
`bytemuck::cast_slice` turns into the bytes of an instance buffer. Writing that
buffer, binding it, sorting it and drawing it are all the game's, which is why
the whole crate runs in a test with no screen attached and why a particle effect
here is something a golden test can hold to an exact answer.

## Seeded, and never a clock

This is the client ring, so nothing here is hashed, nothing here is sent, and
two machines are allowed to disagree about every particle on their screens.
That is what lets it be floating point.

It is not what lets it be unrepeatable, and the difference is the subtle one.
Every random number a system draws comes from the [`Rng`] seeded in
[`System::new`], and every step takes the duration the caller passes it. Nothing
in this crate asks what time it is. So the same emitters, the same bursts and
the same sequence of `dt` produce the same particles in the same order on the
same build, every time, and a run can be frozen as a golden and compared against
a later one.

```rust
use corvid_glm::Vec3;
use corvid_particle::{Emitter, Instance, Shape, System};

fn run(seed: u64) -> Vec<Instance> {
    let mut system = System::new(256, seed);
    let smoke = system.add(Emitter::new(Vec3::zeros(), Shape::Sphere { radius: 1.0 }));
    system.burst(smoke, 32).expect("just added");
    for _ in 0..10 {
        system.step(1.0 / 60.0);
    }
    system.instances().collect()
}

assert_eq!(run(1789), run(1789));
assert_ne!(run(1789), run(1790));
```

What repeats is a run, not a machine: an `f32` multiply is the hardware's, and
this crate makes no claim that two architectures agree on the last bit. The
claim it does make is stronger than it sounds, because the usual reason a
particle system cannot repeat is that it seeded itself from the clock and drew
its numbers on a thread the frame rate decides the schedule of. Neither happens
here.

## What an emitter can say

An [`Emitter`] is a place, a [`Shape`] that offsets a particle from it and
points it somewhere, a speed, a lifetime, a size that grows or shrinks, a spin,
a [`ColorRamp`] over the whole life, an acceleration, a drag and an optional
[`Trail`]. It is a record with public fields and no invariants: every value is
defined, so an editor can move any of them to any number without this crate
having an opinion about it.

Four of the things the fire and explosion designs ask for come straight
out of those fields. Continuous smoke off a
burning wall is a [`Shape::Cone`] pointed up with a
[`rate`](Emitter::rate), a long lifetime, a large
[`size_end`](Emitter::size_end) and a small positive
[`gravity`](Emitter::gravity), which is what buoyancy is when the only
force in the crate is a constant one. The core of an explosion is a
[`Shape::Sphere`] burst all at once. A shockwave along the ground is a
[`Shape::Ring`] whose normal is the world's up, where every particle leaves
along its own radius and the ring expands as a ring. Embers are a burst with a
real [`gravity`](Emitter::gravity) and a [`drag`](Emitter::drag), which
together give them a terminal velocity rather than an ever-steeper fall.

Shrapnel is the fifth and it is the one that needs a second emitter. A
[`Trail`] names another emitter in the same system, and every live particle of
the one carrying it becomes an emitter of the other -- so the fragments are one
burst and the smoke marking where each of them went is another. A trail's
particles are spread along the segment their parent crossed during the step
rather than all being born where it ended up, which is the difference between a
line and a row of beads at thirty metres a second.

## Motion, and why it is arithmetic

A particle is a position and a velocity under `dv/dt = gravity - drag * v`,
stepped as

```text
v <- (v + gravity * dt) / (1 + drag * dt)
x <- x + v * dt
```

The drag is taken implicitly -- the divisor, rather than a multiply by
`1 - drag * dt` -- and that is not a detail. The explicit form oscillates once
`drag * dt` passes one and diverges once it passes two, and an ember in air is a
drag of two per second, so a machine dropping to four frames a second would
throw its embers into the sky. This form cannot: the divisor is never below one.

It is also exactly composable, so what a hundred steps do is arithmetic rather
than a previous run. With `terminal = gravity / drag` and
`decay = 1 / (1 + drag * dt)`, `n` steps give
`v(n) = terminal + (v0 - terminal) * decay^n` and
`x(n) = x0 + terminal * n * dt + (v0 - terminal) * (1 - decay^n) / drag`;
with no drag they give the exact ballistic `v0 + gravity * n * dt`. `tests/motion.rs`
holds a particle against both over a hundred steps.

## The budget

A city on fire is the load case, so a [`System`] has a hard cap on live
particles, taken in [`System::new`] and allocated there. When a full system is
asked for one more, **the oldest dies**.

Not the newest, which would be cheaper still. The newest particle is the burst
that just went off -- the thing the player is looking at, and the reason the
pool overflowed -- and a system that refuses it goes on drawing what has already
happened while ignoring what is happening. The oldest is the most faded, because
a ramp ends at nothing, so losing it shortens the tail of an effect instead of
removing its head. The pool is a queue in birth order for exactly this reason:
the oldest is at the front, and the policy costs a `pop_front` rather than a
scan.

[`System::dropped`] counts every particle the budget has refused, evicted or
never born. It is a counter rather than a log line because the thing that asks
is a test and a frame graph rather than a person, and a number that climbs while
the district burns says the cap is the wrong size.

## Sorting is the caller's problem

The instances come out oldest first, which is the order they were born in. It is
not the order alpha blending needs. Additive passes -- flame, embers, anything
drawn as light -- do not care, and that is most of what this crate is for.
Soft smoke drawn with straight alpha does care, and the caller sorts it by view
depth before writing the buffer, because this crate does not know where the
camera is and is not going to guess.

## Scope

It will cover what one emitter can describe about one particle: where it starts,
how it moves under a constant acceleration and a linear drag, how big it is, how
it is turned, what colour it is over its life, and one level of trailing. It
will grow the pieces the fire and explosion designs turn out to need in that
shape.

It will not cover collision, because a particle that bounces has to ask the
world what it hit and this crate has no world. It will not cover attraction,
turbulence or curl noise, because a field is something a game already has and
passing it in would make an emitter a closure rather than a record. It will not
sort, batch, cull or draw, and it will not learn what a texture is: an
[`Instance`] carries no atlas index because which of the three passes of the
design a particle belongs to is a property of the system it came out of and not
of the particle, so a game runs one system per pass and knows which is which.
There is no `serde` feature, because an [`Emitter`] holds a `corvid_glm::Vec3`
and giving that an encoding is `corvid_glm`'s decision rather than this crate's;
a game keeping effects in a data pack writes its own record and builds an
[`Emitter`] from it.

Nothing here will ever be hashed or sent. If a particle ever needs to be, it has
stopped being a particle and become a projectile, and a projectile belongs in
the simulation ring where the arithmetic is fixed point.
