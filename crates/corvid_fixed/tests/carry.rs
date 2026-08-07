//! That a carried velocity moves at that velocity, on average, forever.
//!
//! The claim under all of this is one sentence: **after any sequence of steps
//! the total displacement differs from the exact total by less than one
//! representable step** — not per step, in total, however many steps there
//! were. Everything below is that sentence at a different velocity, a different
//! rate, or a different type.
//!
//! The reference is computed in `i128` from the same integers, so it is exact
//! rather than a floating-point approximation of what was wanted. A test whose
//! reference is itself rounded cannot see a rounding bug.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::cast_precision_loss,
    reason = "the ratios printed in failure messages are for a person to read"
)]

use core::time::Duration;

use corvid_fixed::{Angle32, Carry, Factor16, Fixed, I16F16, I24F8, Signed16};

/// The exact displacement of `frames` steps of `micros` at `velocity`, in bits.
///
/// Integer arithmetic, so this is the number the carry is being compared
/// against rather than a rounding of it.
const fn exact(velocity: i128, micros: i128, frames: i128) -> i128 {
    velocity * micros * frames / 1_000_000
}

/// What a carry actually delivers over `frames` steps.
fn walked<T: Fixed>(velocity: T, micros: u64, frames: usize) -> i128
where
    T::Bits: Into<i128>,
{
    let dt = Duration::from_micros(micros);
    let mut carry = Carry::<T>::ZERO;
    let mut total = 0i128;
    for _ in 0..frames {
        total += carry.step(velocity, dt).to_bits().into();
    }
    total
}

#[test]
fn the_total_is_within_one_step_of_exact_at_every_rate() {
    // The claim, at the rates a display actually runs and at two that no
    // display runs — 1000 Hz because a frame there is a fraction of one
    // representable step, and 3 Hz because a frame there is hundreds of them.
    let velocity = I24F8::from_f64(4.0);
    for hz in [3u64, 15, 30, 60, 72, 90, 120, 144, 165, 240, 360, 1_000] {
        let micros = 1_000_000 / hz;
        let frames = usize::try_from(hz * 60).expect("a minute of frames");
        let walked = walked(velocity, micros, frames);
        let exact = exact(
            i128::from(velocity.to_bits()),
            i128::from(micros),
            i128::try_from(frames).expect("a minute of frames"),
        );
        assert!(
            (walked - exact).abs() <= 1,
            "a minute at {hz} Hz walked {walked} against an exact {exact}",
        );
    }
}

#[test]
fn a_million_frames_do_not_accumulate_anything() {
    // The property that makes this worth having rather than "rounding to
    // nearest is close enough". A per-step error of even one part in a million
    // would show up here as a whole step; a *constant* per-step error — which
    // is what truncation gives for a held velocity — would show up as a
    // million of them.
    //
    // The velocity and the frame time are chosen to be maximally unfriendly:
    // 6944 µs is 144 Hz truncated, and this velocity's product with it lands
    // 0.99968 of the way through a step.
    let velocity = I24F8::from_bits(1_000);
    let frames = 1_000_000;
    let walked = walked(velocity, 6_944, frames);
    let exact = exact(1_000, 6_944, i128::try_from(frames).unwrap());
    assert!(
        (walked - exact).abs() <= 1,
        "a million frames walked {walked} against an exact {exact}, \
         which is {:.3} steps of drift",
        (walked - exact) as f64,
    );
}

#[test]
fn truncating_would_have_failed_the_test_above() {
    // The control. Without this, every assertion in this file would also pass
    // against an implementation that simply did not round badly enough to
    // notice at these numbers — so here is what the naive version gives, at the
    // same velocity and rate, stated as the thing being avoided.
    let velocity = 1_000i128;
    let micros = 6_944i128;
    let frames = 1_000_000i128;

    // What one step is worth, truncated the way a `from_bits` of a divided
    // product would truncate it.
    let per_step = velocity * micros / 1_000_000;
    let truncated = per_step * frames;
    let exact = exact(velocity, micros, frames);

    assert!(
        (truncated - exact).abs() > 900_000,
        "truncation lost {} steps, which is not enough to be worth a test",
        exact - truncated,
    );
    // 6.944 becomes 6: 13.6% of the distance, gone, and every frame in the
    // same direction.
    assert_eq!(per_step, 6);
}

#[test]
fn a_step_is_always_the_floor_or_the_ceiling_of_the_exact_one() {
    // What "not a filter" means. The carry never hands back a smoothed value:
    // each step is one of the two representable values the exact displacement
    // sits between, so a velocity that steps cleanly steps cleanly every time
    // and nothing is ever invented.
    let velocity = I24F8::from_f64(1.5);
    let dt = Duration::from_micros(6_944);
    let per_step = i128::from(velocity.to_bits()) * 6_944 / 1_000_000;
    let mut carry = Carry::<I24F8>::ZERO;
    for _ in 0..10_000 {
        let step = i128::from(carry.step(velocity, dt).to_bits());
        assert!(
            step == per_step || step == per_step + 1,
            "a step of {step} is neither {per_step} nor {}",
            per_step + 1,
        );
    }
}

#[test]
fn the_two_directions_are_exactly_symmetric() {
    // The test that settled how the division rounds. A *floor* bounds the total
    // error exactly as well as truncation does — both are within one step — and
    // it is not symmetric: at one bit per second it walks 69 steps forwards and
    // 70 backwards, because "down" is towards the destination one way and away
    // from it the other. Truncation towards zero makes the computation odd, so
    // the two directions mirror.
    //
    // No player could find that asymmetry, and every long run would carry it.
    for bits in [1, 7, 1_000, 32_767] {
        let forward = walked(I24F8::from_bits(bits), 6_944, 10_000);
        let backward = walked(I24F8::from_bits(-bits), 6_944, 10_000);
        assert_eq!(
            forward, -backward,
            "{bits} bits/s walked {forward} forwards and {backward} backwards",
        );
    }
}

#[test]
fn a_journey_out_and_back_lands_where_it_started() {
    // What the symmetry buys, as a thing a player would notice: strafing left
    // for a while and right for the same while puts you back on the step you
    // set off from, rather than a few millimetres along.
    let dt = Duration::from_micros(6_944);
    let velocity = I24F8::from_bits(997);
    let mut carry = Carry::<I24F8>::ZERO;
    let mut here = 0i128;
    for _ in 0..5_000 {
        here += i128::from(carry.step(velocity, dt).to_bits());
    }
    for _ in 0..5_000 {
        here += i128::from(carry.step(-velocity, dt).to_bits());
    }
    assert_eq!(here, 0, "the round trip drifted {here} steps");
}

#[test]
fn letting_go_and_pressing_again_covers_the_same_ground() {
    // What the remainder surviving a zero velocity buys. A player tapping a
    // direction and a player holding it for the same total time should arrive
    // in the same place — a carry that reset when the key came up would round
    // the tail of every tap away, and a game played in taps would be slower
    // than the same game played in holds.
    let velocity = I24F8::from_bits(1_000);
    let dt = Duration::from_micros(6_944);

    let held = walked(velocity, 6_944, 2_000);

    let mut carry = Carry::<I24F8>::ZERO;
    let mut tapped = 0i128;
    for frame in 0..4_000 {
        // On for one frame, off for one, so half the frames move.
        let pressed = if frame % 2 == 0 {
            velocity
        } else {
            I24F8::ZERO
        };
        tapped += i128::from(carry.step(pressed, dt).to_bits());
    }

    assert!(
        (held - tapped).abs() <= 1,
        "holding walked {held} and tapping walked {tapped}",
    );
}

#[test]
fn a_carry_that_is_reset_starts_over() {
    // The one way to lose the remainder deliberately: something that was
    // teleported rather than moved has a debt belonging to a journey that is no
    // longer happening.
    let velocity = I24F8::from_bits(999);
    let dt = Duration::from_millis(1);
    let mut carry = Carry::<I24F8>::ZERO;
    let _ = carry.step(velocity, dt);
    assert_ne!(carry.owed(), 0, "there is something to lose");
    carry.reset();
    assert_eq!(carry.owed(), 0);
    assert_eq!(carry, Carry::ZERO);
}

#[test]
fn the_remainder_is_never_negative_and_never_reaches_a_whole_step() {
    // The internal invariant the exactness rests on, checked at both signs.
    let dt = Duration::from_micros(6_944);
    for bits in [-32_768, -1_000, -1, 0, 1, 1_000, 32_767] {
        let mut carry = Carry::<I16F16>::ZERO;
        for _ in 0..1_000 {
            let _ = carry.step(I16F16::from_bits(bits), dt);
            // Signed, and strictly inside a whole step either way. The sign
            // follows the direction of travel, which is what makes the two
            // directions mirror.
            assert!(
                (-999_999..=999_999).contains(&carry.owed()),
                "{bits} bits/s left {} owed",
                carry.owed(),
            );
            assert!(
                carry.owed() * i64::from(bits.signum()) >= 0,
                "{bits} bits/s owes {} against its direction",
                carry.owed(),
            );
        }
    }
}

#[test]
fn a_zero_step_moves_nothing_and_forgets_nothing() {
    let mut carry = Carry::<I24F8>::ZERO;
    let velocity = I24F8::from_bits(1_000);
    let _ = carry.step(velocity, Duration::from_micros(6_944));
    let owed = carry.owed();
    assert_eq!(carry.step(velocity, Duration::ZERO), I24F8::ZERO);
    assert_eq!(carry.owed(), owed);
}

#[test]
fn an_absurd_step_pins_rather_than_wrapping() {
    // A duration no frame has, which is what a debugger paused for an hour
    // hands the next frame. The displacement saturates at the type's bound
    // rather than wrapping through it, which is a thing at the far wall rather
    // than a thing behind you.
    let mut carry = Carry::<I24F8>::ZERO;
    let moved = carry.step(I24F8::from_f64(1_000.0), Duration::from_hours(24));
    assert_eq!(moved, I24F8::MAX);

    let back = carry.step(I24F8::from_f64(-1_000.0), Duration::from_hours(24));
    assert_eq!(back, I24F8::MIN);
}

#[test]
fn it_carries_every_family_and_not_only_a_position() {
    // What the trait is for. A velocity is "so many of this type per second"
    // whatever the type means, so an angle a turret slews through and a factor
    // a fade runs at integrate exactly as a position does.
    let dt = Duration::from_micros(6_944);

    // A sixth of a turn a second, which is where a menu-driven camera sits.
    let mut turret = Carry::<Angle32>::ZERO;
    let slew = Angle32::from_turns(1.0 / 6.0);
    let mut turned = 0i128;
    for _ in 0..144 {
        turned += i128::from(turret.step(slew, dt).to_bits());
    }
    let exact = exact(i128::from(slew.to_bits()), 6_944, 144);
    assert!((turned - exact).abs() <= 1, "{turned} against {exact}");

    // And the two narrow families, which is where a per-step rounding error
    // would be largest relative to the value.
    let mut fade = Carry::<Factor16>::ZERO;
    assert_eq!(fade.step(Factor16::ZERO, dt), Factor16::ZERO);
    let mut push = Carry::<Signed16>::ZERO;
    assert_eq!(push.step(Signed16::ZERO, dt), Signed16::ZERO);
}

#[test]
fn several_axes_do_not_pay_each_others_debts() {
    // One carry per axis, and the reason: a single shared remainder would let
    // motion along one axis spend what another owed, which bends a straight
    // line into a staircase. A velocity with a zero component must stay exactly
    // zero along it however long the run is.
    let mut carry = [Carry::<I24F8>::ZERO; 3];
    let velocity = [I24F8::from_bits(1_000), I24F8::ZERO, I24F8::from_bits(999)];
    let dt = Duration::from_micros(6_944);

    let mut total = [0i128; 3];
    for _ in 0..10_000 {
        let step = Carry::step_each(&mut carry, velocity, dt);
        for (sum, moved) in total.iter_mut().zip(step) {
            *sum += i128::from(moved.to_bits());
        }
    }

    assert_eq!(total[1], 0, "the axis nobody moved along moved");
    for (axis, speed) in total.iter().zip(velocity) {
        let exact = exact(i128::from(speed.to_bits()), 6_944, 10_000);
        assert!((axis - exact).abs() <= 1, "{axis} against {exact}");
    }
}
