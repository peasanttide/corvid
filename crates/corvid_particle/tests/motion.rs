//! One particle, a hundred steps, against arithmetic.
//!
//! The point of holding the integrator to a closed form rather than to a
//! recorded run is that a recorded run agrees with whatever the code did,
//! including the day it starts doing something else. These two formulas are
//! the solution of `dv/dt = gravity - drag * v` under the exact scheme
//! `v <- (v + gravity * dt) / (1 + drag * dt)`, `x <- x + v * dt`, so a step
//! that changed would have to change them too, and changing them is a thing a
//! reader can see.
//!
//! The physical values are written as `f64` and narrowed with
//! [`corvid_float::demote`] where the system takes them, so the arithmetic
//! below is the double-precision answer and what it is compared against is the
//! single-precision run. The tolerance is what a hundred single-precision steps
//! cost rather than slack in the claim: `tests/determinism.rs` is where
//! exactness is checked, and it is checked there as bytes.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_float::demote;
use corvid_glm::Vec3;
use corvid_particle::{Emitter, Range, Shape, System};

/// The step the arithmetic below is written for.
const DT: f64 = 1.0 / 120.0;

/// How many of them.
const STEPS: i32 = 100;

/// What a hundred single-precision steps are allowed to differ from the
/// arithmetic by, in metres. Two tenths of a millimetre over two metres of
/// travel.
const TOLERANCE: f64 = 2e-4;

/// The acceleration on an ember, in metres a second squared.
const GRAVITY: f64 = -9.81;

/// A system holding one particle, launched along `+X` at `speed`.
///
/// A cone of no spread is the one shape whose direction is exact: the sampling
/// takes the axis itself, so the initial velocity is `speed` along `+X` and
/// nothing else -- which is what makes a closed form comparable at all.
fn launched(speed: f64, gravity: f64, drag: f64) -> System {
    let mut system = System::new(4, 1789);
    let mut emitter = Emitter::new(
        Vec3::zeros(),
        Shape::Cone {
            axis: Vec3::new(1.0, 0.0, 0.0),
            spread: 0.0,
        },
    );
    emitter.speed = Range::exactly(demote(speed));
    emitter.lifetime = Range::exactly(1000.0);
    emitter.gravity = Vec3::new(0.0, 0.0, demote(gravity));
    emitter.drag = demote(drag);
    let id = system.add(emitter);
    system.burst(id, 1).expect("the emitter is live");
    system
}

/// Where the one particle in a system is.
fn position(system: &System) -> [f64; 3] {
    let instance = system.instances().next().expect("one particle");
    instance.position.map(f64::from)
}

/// Steps a system by [`DT`], `count` times.
fn run(system: &mut System, count: i32) {
    for _ in 0..count {
        system.step(demote(DT));
    }
}

/// A ballistic arc, with no air in it.
///
/// `x(n) = x0 + v0 * n * dt + gravity * dt^2 * n * (n + 1) / 2`. The half step
/// in `n * (n + 1)` rather than `n * n` is the price of taking the end-of-step
/// velocity, and it is in the formula rather than in the tolerance because it
/// is arithmetic rather than error.
#[test]
fn gravity_alone_is_ballistic() {
    let speed = 5.0;
    let mut system = launched(speed, GRAVITY, 0.0);
    run(&mut system, STEPS);

    let n = f64::from(STEPS);
    let expected_x = speed * n * DT;
    let expected_z = GRAVITY * DT * DT * n * (n + 1.0) / 2.0;

    let [x, y, z] = position(&system);
    assert!(
        (x - expected_x).abs() < TOLERANCE,
        "{x} against {expected_x}"
    );
    assert!(
        y.abs() < TOLERANCE,
        "{y} is not on the plane it was launched in"
    );
    assert!(
        (z - expected_z).abs() < TOLERANCE,
        "{z} against {expected_z}"
    );
}

/// The same arc through air, which is the ember.
///
/// `x(n) = x0 + terminal * n * dt + (v0 - terminal) * (1 - decay^n) / drag`,
/// with `terminal = gravity / drag` and `decay = 1 / (1 + drag * dt)`.
#[test]
fn gravity_and_drag_are_a_closed_form() {
    let drag = 2.0;
    let speed = 5.0;
    let mut system = launched(speed, GRAVITY, drag);
    run(&mut system, STEPS);

    let n = f64::from(STEPS);
    let decay = (1.0 / (1.0 + drag * DT)).powf(n);

    // Along the launch, where the terminal velocity is nothing at all and the
    // whole of the motion is the initial speed being eaten by the air.
    let expected_x = speed * (1.0 - decay) / drag;
    // And down, where it is not.
    let terminal = GRAVITY / drag;
    let expected_z = terminal * n * DT + (0.0 - terminal) * (1.0 - decay) / drag;

    let [x, y, z] = position(&system);
    assert!(
        (x - expected_x).abs() < TOLERANCE,
        "{x} against {expected_x}"
    );
    assert!(
        y.abs() < TOLERANCE,
        "{y} is not on the plane it was launched in"
    );
    assert!(
        (z - expected_z).abs() < TOLERANCE,
        "{z} against {expected_z}"
    );
}

/// Given long enough, an ember falls at `gravity / drag` and no faster.
///
/// The thing a linear drag is in the crate for. Measured as the distance one
/// step covers, which is the only way a velocity can be seen from outside --
/// [`Instance`](corvid_particle::Instance) carries no velocity, on purpose.
#[test]
fn drag_gives_a_terminal_velocity() {
    let drag = 2.0;
    let mut system = launched(0.0, GRAVITY, drag);
    run(&mut system, 1200);

    let before = position(&system)[2];
    run(&mut system, 1);
    let after = position(&system)[2];

    let terminal = GRAVITY / drag;
    let measured = (after - before) / DT;
    assert!(
        (measured - terminal).abs() < 1e-3,
        "{measured} against {terminal}"
    );
}

/// A drag with no gravity behind it is a particle coasting to a stop, and it
/// never quite arrives.
///
/// What the implicit scheme buys, and the reason it is the one here: an
/// explicit `v * (1 - drag * dt)` at this drag and this step would send the
/// particle backwards on the first step and further backwards on every step
/// after it.
#[test]
fn a_large_drag_is_stable() {
    let mut system = launched(20.0, 0.0, 200.0);
    let mut previous = position(&system)[0];
    for _ in 0..STEPS {
        run(&mut system, 1);
        let now = position(&system)[0];
        assert!(now >= previous, "went backwards: {now} after {previous}");
        assert!(
            now < 1.0,
            "went a long way for a drag of two hundred: {now}"
        );
        previous = now;
    }
}

/// A step that is not a positive finite number moves nothing.
#[test]
fn a_useless_step_does_nothing() {
    let mut system = launched(5.0, GRAVITY, 0.0);
    let start = position(&system);
    for dt in [0.0, -1.0 / 60.0, f32::NAN, f32::INFINITY] {
        system.step(dt);
    }
    let [x, y, z] = position(&system);
    assert!((x - start[0]).abs() < f64::EPSILON, "{x} moved");
    assert!((y - start[1]).abs() < f64::EPSILON, "{y} moved");
    assert!((z - start[2]).abs() < f64::EPSILON, "{z} moved");
    assert_eq!(system.len(), 1);
}
