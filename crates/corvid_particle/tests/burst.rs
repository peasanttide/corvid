//! A burst emits its stated count and no more, and an id that names nothing
//! says so.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_glm::Vec3;
use corvid_particle::{Emitter, ParticleError, Range, Shape, System};

/// An emitter that makes one particle per burst count and nothing on its own.
fn silent() -> Emitter {
    let mut emitter = Emitter::new(Vec3::zeros(), Shape::Point);
    emitter.lifetime = Range::exactly(10.0);
    emitter
}

/// The count is the count.
#[test]
fn a_burst_is_its_count() {
    let mut system = System::new(1024, 1789);
    let id = system.add(silent());

    system.burst(id, 200).expect("the emitter is live");
    assert_eq!(system.len(), 200);
    assert_eq!(system.instances().count(), 200);
    assert_eq!(system.dropped(), 0);
}

/// And nothing else adds to it: an emitter with no rate makes nothing when the
/// system is stepped, however long it is stepped for.
#[test]
fn a_rateless_emitter_emits_only_on_burst() {
    let mut system = System::new(1024, 1789);
    let id = system.add(silent());

    system.burst(id, 7).expect("the emitter is live");
    for _ in 0..100 {
        system.step(1.0 / 60.0);
    }
    assert_eq!(system.len(), 7);
}

/// Bursts add rather than replace.
#[test]
fn bursts_accumulate() {
    let mut system = System::new(1024, 1789);
    let id = system.add(silent());

    system.burst(id, 10).expect("the emitter is live");
    system.burst(id, 10).expect("the emitter is live");
    assert_eq!(system.len(), 20);
}

/// A count of zero is a burst of nothing rather than an error, because a
/// caller computing a count from a mass has no reason to special-case the one
/// where it comes out at nothing.
#[test]
fn a_burst_of_none_is_allowed() {
    let mut system = System::new(1024, 1789);
    let id = system.add(silent());

    system.burst(id, 0).expect("the emitter is live");
    assert!(system.is_empty());
}

/// A removed emitter cannot be burst from, and neither can a stale id whose
/// slot has been taken over by a later add.
///
/// The second half is the one the generation in an
/// [`corvid_particle::EmitterId`] is for: without it the stale id would name
/// the new emitter and the burst would come out of the wrong place.
#[test]
fn a_stale_id_is_refused() {
    let mut system = System::new(1024, 1789);
    let first = system.add(silent());

    system.remove(first).expect("the emitter is live");
    assert_eq!(
        system.burst(first, 1),
        Err(ParticleError::UnknownEmitter(first))
    );
    assert_eq!(system.get(first), Err(ParticleError::UnknownEmitter(first)));

    // The slot is free -- nothing was ever emitted from it -- so this takes it
    // over, and the id from before still has to be refused.
    let second = system.add(silent());
    assert_eq!(
        system.burst(first, 1),
        Err(ParticleError::UnknownEmitter(first))
    );
    system.burst(second, 3).expect("the new emitter is live");
    assert_eq!(system.len(), 3);
}

/// An id is a handle to a slot rather than to a system, and this is where that
/// caveat is written down.
///
/// A system with no such slot refuses it, which is what this checks. A system
/// that happened to hold a live emitter at the same index and generation would
/// not, because there is nothing in an index and a generation to tell the two
/// systems apart -- so a caller juggling several systems is the one that has to
/// keep their ids straight.
#[test]
fn an_id_belongs_to_its_system() {
    let mut one = System::new(16, 1);
    let mut other = System::new(16, 2);
    let id = one.add(silent());

    assert_eq!(other.burst(id, 1), Err(ParticleError::UnknownEmitter(id)));
    assert!(other.is_empty());
}
