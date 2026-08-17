//! Shrapnel that trails: one burst of fragments, and a second emitter running
//! from each of them.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_glm::Vec3;
use corvid_particle::{Emitter, EmitterId, Instance, Range, Shape, System, Trail};

/// The size a trail particle is born at, which is how the test tells one from a
/// fragment. Any per-particle number would do; the size is the one that is
/// carried to an instance untouched.
const TRAIL_SIZE: f32 = 2.0;

/// The size a fragment is born at.
const FRAGMENT_SIZE: f32 = 1.0;

/// A system with a trail emitter and one fragment emitter that runs it.
///
/// The fragment goes along `+X` at ten metres a second and lives long enough
/// not to matter. The trail's own particles do not move at all, so where they
/// are is where they were born and the segment they were spread along can be
/// read straight off the instances.
fn shrapnel(rate: f32) -> (System, EmitterId) {
    let mut system = System::new(1024, 1789);

    let mut smoke = Emitter::new(Vec3::zeros(), Shape::Point);
    smoke.speed = Range::exactly(0.0);
    smoke.lifetime = Range::exactly(100.0);
    smoke.size = Range::exactly(TRAIL_SIZE);
    let smoke = system.add(smoke);

    let mut fragment = Emitter::new(
        Vec3::zeros(),
        Shape::Cone {
            axis: Vec3::new(1.0, 0.0, 0.0),
            spread: 0.0,
        },
    );
    fragment.speed = Range::exactly(10.0);
    fragment.lifetime = Range::exactly(100.0);
    fragment.size = Range::exactly(FRAGMENT_SIZE);
    fragment.trail = Some(Trail {
        emitter: smoke,
        rate,
    });
    let fragment = system.add(fragment);

    (system, fragment)
}

/// Everything a system holds that came out of the trail.
fn trailed(system: &System) -> Vec<Instance> {
    system
        .instances()
        .filter(|instance| instance.size > f32::midpoint(TRAIL_SIZE, FRAGMENT_SIZE))
        .collect()
}

/// A fragment leaves the number of particles its rate says, and no others.
#[test]
fn a_fragment_trails_at_its_rate() {
    let (mut system, fragment) = shrapnel(40.0);
    system.burst(fragment, 1).expect("the emitter is live");

    // A quarter of a second at forty a second is ten.
    system.step(0.25);
    assert_eq!(trailed(&system).len(), 10);
    assert_eq!(system.len(), 11, "and the fragment itself");

    // And another quarter is another ten, the fraction carried between steps
    // rather than rounded away.
    system.step(0.25);
    assert_eq!(trailed(&system).len(), 20);
}

/// A rate below one a step still trails, because the debt is carried.
#[test]
fn a_slow_trail_still_trails() {
    let (mut system, fragment) = shrapnel(4.0);
    system.burst(fragment, 1).expect("the emitter is live");

    for _ in 0..16 {
        system.step(1.0 / 60.0);
    }
    assert_eq!(
        trailed(&system).len(),
        1,
        "four a second owes one after a quarter of a second"
    );
}

/// The trail is spread along the segment the fragment crossed, rather than
/// piled up where it ended the step.
///
/// This is the difference between a line and a row of beads. Ten particles
/// over a step in which the fragment travelled two and a half metres land at
/// even quarter-metre intervals inside it, none of them at either end.
#[test]
fn a_trail_is_spread_along_the_segment() {
    let (mut system, fragment) = shrapnel(40.0);
    system.burst(fragment, 1).expect("the emitter is live");
    system.step(0.25);

    let mut xs: Vec<f32> = trailed(&system)
        .iter()
        .map(|instance| instance.position[0])
        .collect();
    xs.sort_by(f32::total_cmp);
    assert_eq!(xs.len(), 10);

    // The fragment covered 0 .. 2.5 metres, so the ten sit at the middles of
    // ten even slices of it.
    for (nth, x) in xs.iter().enumerate() {
        #[expect(
            clippy::cast_precision_loss,
            reason = "ten is exact in an f32 and this is a test index"
        )]
        let expected = 2.5 * (nth as f32 + 0.5) / 10.0;
        assert!((x - expected).abs() < 1e-3, "{x} against {expected}");
    }
}

/// Removing the trail emitter stops the trailing and leaves the fragments
/// flying.
#[test]
fn a_removed_trail_stops_trailing() {
    let (mut system, fragment) = shrapnel(40.0);
    system.burst(fragment, 1).expect("the emitter is live");
    system.step(0.25);
    let before = trailed(&system).len();
    assert_eq!(before, 10);

    // The trail emitter is the first one added, and the fragment names it.
    let smoke = system
        .get(fragment)
        .expect("the emitter is live")
        .trail
        .expect("the fragment trails")
        .emitter;
    system.remove(smoke).expect("the emitter is live");

    system.step(0.25);
    assert_eq!(trailed(&system).len(), before, "no new trail particles");
    assert!(
        system.instances().any(|i| i.position[0] > 4.0),
        "the fragment carried on"
    );
}

/// A trail obeys the budget like anything else.
///
/// The fragment is the oldest thing in the pool, so it is what the cap takes
/// first and its trail is what survives it -- which is the drop policy working
/// as written and is documented on
/// [`Trail`](corvid_particle::Trail).
#[test]
fn a_trail_is_capped_too() {
    let (mut system, fragment) = shrapnel(400.0);
    system.burst(fragment, 1).expect("the emitter is live");

    for _ in 0..600 {
        system.step(1.0 / 60.0);
        assert!(system.len() <= 1024);
    }
    assert_eq!(system.len(), 1024);
    assert!(system.dropped() > 0);
}
