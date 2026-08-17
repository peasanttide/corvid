//! A full system drops by policy rather than growing.
//!
//! The load case is a district on fire: a hundred burning parts, every one of
//! them emitting, for as long as the level lasts. What has to be true then is
//! that the cost is the cap and not the fire.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_glm::Vec3;
use corvid_particle::{Emitter, Range, Shape, System};

/// An emitter at `x` whose particles stay where they were born and last for
/// ever, so that a survivor can be told from a casualty by looking at it.
fn marked(x: f32) -> Emitter {
    let mut emitter = Emitter::new(Vec3::new(x, 0.0, 0.0), Shape::Point);
    emitter.speed = Range::exactly(0.0);
    emitter.lifetime = Range::exactly(1000.0);
    emitter
}

/// The oldest dies, and it is the oldest that dies.
///
/// Two emitters, told apart by where they put their particles: eight from the
/// first fill a pool of eight, and eight from the second have to replace all of
/// them. If the policy were the other way round the pool would still hold
/// eight and they would all be the first emitter's.
#[test]
fn the_oldest_dies() {
    const CAP: usize = 8;
    let mut system = System::new(CAP, 1789);
    assert_eq!(system.capacity(), CAP);
    let old = system.add(marked(-100.0));
    let new = system.add(marked(100.0));

    system.burst(old, 8).expect("the emitter is live");
    assert_eq!(system.len(), CAP);
    assert_eq!(system.dropped(), 0);

    system.burst(new, 8).expect("the emitter is live");
    assert_eq!(system.len(), CAP, "the cap is the cap");
    assert_eq!(system.dropped(), 8, "and eight had to go");
    assert!(
        system
            .instances()
            .all(|instance| instance.position[0] > 0.0),
        "the survivors are the new ones"
    );
}

/// Half of them, so that the boundary is checked rather than the extreme.
#[test]
fn only_as_many_as_it_takes_die() {
    const CAP: usize = 8;
    let mut system = System::new(CAP, 1789);
    let old = system.add(marked(-100.0));
    let new = system.add(marked(100.0));

    system.burst(old, 8).expect("the emitter is live");
    system.burst(new, 3).expect("the emitter is live");

    assert_eq!(system.len(), CAP);
    assert_eq!(system.dropped(), 3);
    let survivors = system.instances().filter(|i| i.position[0] < 0.0).count();
    assert_eq!(survivors, 5, "the five youngest of the first burst");
}

/// A hundred emitters running flat out for ten seconds cost the cap and not a
/// particle more, however many particles they describe between them.
#[test]
fn a_district_on_fire_costs_the_cap() {
    const CAP: usize = 2048;
    let mut system = System::new(CAP, 1789);

    for part in 0..100_i16 {
        let mut smoke = Emitter::new(
            Vec3::new(f32::from(part), 0.0, 0.0),
            Shape::Cone {
                axis: Vec3::new(0.0, 0.0, 1.0),
                spread: 0.3,
            },
        );
        smoke.rate = 40.0;
        smoke.lifetime = Range::new(2.0, 4.0);
        smoke.gravity = Vec3::new(0.0, 0.0, 0.4);
        smoke.drag = 0.5;
        let _ = system.add(smoke);
    }

    for _ in 0..600 {
        system.step(1.0 / 60.0);
        assert!(system.len() <= CAP, "over the cap at {}", system.len());
    }
    assert_eq!(system.len(), CAP, "and full, at four thousand a second");
    assert!(
        system.dropped() > 30_000,
        "the counter is what says the cap is too small: {}",
        system.dropped()
    );
}

/// A system with no room at all refuses everything and says how much.
///
/// A legal way to switch an effect off without unwiring it, which is why it is
/// a cap of zero rather than an error.
#[test]
fn a_cap_of_nothing_refuses_everything() {
    let mut system = System::new(0, 1789);
    let id = system.add(marked(0.0));

    system.burst(id, 50).expect("the emitter is live");
    assert!(system.is_empty());
    assert_eq!(system.dropped(), 50);
    assert_eq!(system.instances().count(), 0);
}

/// The budget does not leak the slots of the particles it drops.
///
/// An evicted particle lets go of its emitter exactly as a dead one does, so a
/// retired emitter's slot comes back even when every particle it made was
/// thrown away rather than buried.
#[test]
fn an_evicted_particle_releases_its_emitter() {
    let mut system = System::new(4, 1789);
    let first = system.add(marked(-1.0));
    system.burst(first, 4).expect("the emitter is live");
    system.remove(first).expect("the emitter is live");

    let second = system.add(marked(1.0));
    system.burst(second, 4).expect("the emitter is live");
    assert_eq!(system.len(), 4);
    assert_eq!(system.dropped(), 4);
    assert!(
        system
            .instances()
            .all(|instance| instance.position[0] > 0.0)
    );
}
