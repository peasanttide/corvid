//! A seeded run repeats, exactly, as bytes.
//!
//! This is the property the crate exists for. Everything else here could be
//! written against a floating-point tolerance; this one is compared as the
//! bytes of the instance buffer, because a tolerance would pass a system that
//! had started drawing its numbers from somewhere else and only agreed with
//! itself to four decimal places.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_glm::Vec3;
use corvid_particle::{Emitter, Instance, Range, Shape, System};

/// A system with one continuous emitter and one burst emitter, stepped for
/// half a second at sixty hertz.
///
/// Both kinds on purpose: a rate accumulates a fraction between steps and a
/// burst does not, so a run that only ever burst would not notice an
/// accumulator that had been reset.
fn run(seed: u64) -> Vec<Instance> {
    let mut system = System::new(1024, seed);

    let mut smoke = Emitter::new(
        Vec3::new(1.0, 2.0, 0.0),
        Shape::Cone {
            axis: Vec3::new(0.0, 0.0, 1.0),
            spread: 0.4,
        },
    );
    smoke.rate = 60.0;
    smoke.speed = Range::new(0.5, 2.0);
    smoke.lifetime = Range::new(0.3, 0.8);
    smoke.gravity = Vec3::new(0.0, 0.0, 0.5);
    smoke.drag = 0.7;
    let smoke = system.add(smoke);

    let mut sparks = Emitter::new(Vec3::zeros(), Shape::Sphere { radius: 0.25 });
    sparks.speed = Range::new(2.0, 9.0);
    sparks.lifetime = Range::new(0.2, 1.2);
    sparks.gravity = Vec3::new(0.0, 0.0, -9.81);
    sparks.drag = 2.0;
    sparks.spin = Range::new(-6.0, 6.0);
    let sparks = system.add(sparks);

    for tick in 0..30 {
        if tick % 10 == 0 {
            system.burst(sparks, 40).expect("the emitter is live");
        }
        system.step(1.0 / 60.0);
    }
    // Read through the continuous emitter too, so a run that lost it would not
    // pass on the burst alone.
    assert!(system.get(smoke).is_ok());
    system.instances().collect()
}

/// The same seed, twice, byte for byte.
#[test]
fn a_seed_repeats() {
    let first = run(17_890_428);
    let second = run(17_890_428);
    assert!(!first.is_empty(), "the run produced nothing to compare");
    assert_eq!(
        bytemuck::cast_slice::<Instance, u8>(&first),
        bytemuck::cast_slice::<Instance, u8>(&second),
    );
}

/// A different seed does not.
///
/// Weak on its own -- two streams could agree by accident -- and it is here so
/// that the test above cannot pass by producing nothing, or by producing the
/// same thing whatever it was told.
#[test]
fn another_seed_differs() {
    assert_ne!(run(17_890_428), run(17_890_427));
}

/// A system's stream is its own: two systems seeded alike and stepped alike
/// agree even when a third is being stepped between them.
///
/// The failure this catches is a shared generator, which is what a crate that
/// reached for a thread-local or a static would have. It is also what a clock
/// would look like from here, since a clock is a value that arrives from
/// outside the seed.
#[test]
fn systems_do_not_share_a_stream() {
    let alone = run(1789);

    let mut noise = System::new(64, 12_345);
    let racket = noise.add(Emitter::new(Vec3::zeros(), Shape::Point));
    noise.burst(racket, 32).expect("the emitter is live");
    noise.step(1.0 / 60.0);

    let beside = run(1789);
    assert_eq!(alone, beside);
}
