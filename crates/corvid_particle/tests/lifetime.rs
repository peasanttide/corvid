//! A particle past its lifetime is gone, and the room it was in comes back.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_glm::Vec3;
use corvid_particle::{Emitter, Range, Shape, System};

/// An emitter whose particles live exactly `seconds` and do not move.
fn lasting(seconds: f32) -> Emitter {
    let mut emitter = Emitter::new(Vec3::zeros(), Shape::Point);
    emitter.lifetime = Range::exactly(seconds);
    emitter.speed = Range::exactly(0.0);
    emitter
}

/// Alive up to the lifetime, gone at it.
///
/// The boundary is closed at the end -- a particle whose age has reached its
/// lifetime is dead rather than in its last frame -- because the alternative
/// leaves a particle drawn at the end of its ramp, which for the ramps this
/// crate is for means an invisible instance that still costs a quad.
#[test]
fn a_lifetime_is_a_deadline() {
    let mut system = System::new(64, 1789);
    let id = system.add(lasting(1.0));
    system.burst(id, 10).expect("the emitter is live");

    system.step(0.5);
    assert_eq!(system.len(), 10);
    system.step(0.4999);
    assert_eq!(system.len(), 10, "still short of a second");
    system.step(0.0002);
    assert!(system.is_empty(), "past a second");
}

/// The age a renderer is handed runs from zero to one over that deadline.
#[test]
fn age_is_the_fraction_of_a_life() {
    let mut system = System::new(64, 1789);
    let id = system.add(lasting(2.0));
    system.burst(id, 1).expect("the emitter is live");

    system.step(0.5);
    let quarter = system.instances().next().expect("one particle");
    assert!((quarter.age - 0.25).abs() < 1e-6, "{}", quarter.age);

    system.step(1.0);
    let three_quarters = system.instances().next().expect("one particle");
    assert!(
        (three_quarters.age - 0.75).abs() < 1e-6,
        "{}",
        three_quarters.age
    );
}

/// The room a dead particle was in is used by the next one.
///
/// What proves it is that the second filling of a full pool drops nothing: the
/// cap is the number of particles that may be alive at once, so a system that
/// counted the dead against it would have had to evict on the way back up, and
/// [`System::dropped`](corvid_particle::System::dropped) would say so.
#[test]
fn a_dead_particle_gives_its_room_back() {
    const CAP: usize = 256;
    let mut system = System::new(CAP, 1789);
    let id = system.add(lasting(1.0));

    for round in 0..4 {
        system.burst(id, 256).expect("the emitter is live");
        assert_eq!(system.len(), CAP, "round {round}");
        assert_eq!(system.dropped(), 0, "round {round}");
        // Past the deadline, so every one of them dies and the pool empties.
        system.step(1.5);
        assert!(system.is_empty(), "round {round}");
    }
}

/// An emitter's slot outlives the emitter, so its particles finish their lives
/// after it has been removed.
///
/// A plume of smoke should not vanish when the wall it came off finishes
/// burning, and the slot is what a live particle reads its drag and its ramp
/// out of -- so removing an emitter cannot be allowed to take the slot away
/// while anything is still looking at it.
#[test]
fn particles_outlive_their_emitter() {
    let mut system = System::new(64, 1789);
    let id = system.add(lasting(1.0));
    system.burst(id, 20).expect("the emitter is live");

    system.remove(id).expect("the emitter is live");
    assert_eq!(system.len(), 20, "the particles are still in the air");

    system.step(0.5);
    assert_eq!(system.instances().count(), 20, "and still drawable");

    system.step(0.6);
    assert!(system.is_empty());

    // With the last of them gone the slot is free, and the next add takes it.
    let next = system.add(lasting(1.0));
    system.burst(next, 5).expect("the new emitter is live");
    assert_eq!(system.len(), 5);
}

/// A lifetime of zero is a particle that is born dead, which is one step long
/// rather than an error or an eternity.
#[test]
fn a_lifetime_of_nothing_is_one_step() {
    let mut system = System::new(64, 1789);
    let id = system.add(lasting(0.0));
    system.burst(id, 4).expect("the emitter is live");

    assert_eq!(system.len(), 4, "born, and drawn once");
    let instance = system.instances().next().expect("one particle");
    assert!(
        (instance.age - 1.0).abs() < f32::EPSILON,
        "at the end of a life it never had: {}",
        instance.age
    );

    system.step(1.0 / 60.0);
    assert!(system.is_empty());
}

/// Clearing a system kills the particles and keeps the emitters.
#[test]
fn clearing_leaves_the_emitters() {
    let mut system = System::new(64, 1789);
    let id = system.add(lasting(10.0));
    system.burst(id, 20).expect("the emitter is live");

    system.clear();
    assert!(system.is_empty());

    system.burst(id, 3).expect("the emitter is still live");
    assert_eq!(system.len(), 3);
}
