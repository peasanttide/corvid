//! The slow end of the range, where the interpolation arithmetic runs closest
//! to its ceiling.
//!
//! `alpha` multiplies the accumulator by 65 535 in a `u64`. That is safe because
//! a span is a `u32` of nanoseconds and the accumulator is below one span -- a
//! bound in the type rather than a habit of callers, which matters because the
//! `serde` feature makes a span `transparent` over that integer, so a save file
//! or a peer supplies one directly and no constructor is in the way.
//!
//! Held to a `u64` of nanoseconds instead, the same product left the integer
//! past about thirty-nine hours of span, and failed differently in the two
//! profiles this workspace ships: a panic where overflow checks are on, and
//! where they are off a factor that was not even monotonic, so a renderer read
//! the picture jumping backwards as the span grew. What is checked here is the
//! whole range the type now admits, ending at the largest span there is.

use core::num::NonZeroU32;
use core::time::Duration;

use corvid_fixed::Factor16;
use corvid_time::{Step, TickSpan};

/// A span from a plain number of nanoseconds, which is the constructor with no
/// rate behind it. Spelled without an `unwrap` so it works in a `const`, as
/// `tests/step.rs` spells its rates.
const fn span(nanos: u32) -> TickSpan {
    match NonZeroU32::new(nanos) {
        Some(nanos) => TickSpan::from_nanos(nanos),
        None => TickSpan::CRADLE,
    }
}

#[test]
fn the_longest_span_is_slower_than_any_rate_can_name() {
    // The bound is worth stating as a value: four seconds and change, where the
    // slowest span `from_hz` can produce is one second. So narrowing the field
    // to a `u32` took nothing away that a rate could have asked for.
    assert_eq!(TickSpan::MAX.nanos(), u32::MAX);
    assert_eq!(TickSpan::MAX.period(), Duration::from_nanos(4_294_967_295));

    let slowest_rate = TickSpan::from_hz(NonZeroU32::MIN);
    assert_eq!(slowest_rate.period(), Duration::from_secs(1));
    assert!(slowest_rate < TickSpan::MAX);
}

#[test]
fn a_four_second_span_still_answers_an_alpha() {
    let mut step = Step::new(TickSpan::MAX);

    // A quarter of the way to the next tick, and a three-quarters that a
    // product leaving its integer got wrong rather than merely imprecise.
    step.advance(Duration::from_nanos(u64::from(u32::MAX) / 4));
    assert_eq!(step.alpha(), Factor16::from_bits(16384));

    step.advance(Duration::from_nanos(u64::from(u32::MAX) / 2));
    assert_eq!(step.alpha(), Factor16::from_bits(49151));
}

#[test]
fn alpha_climbs_with_the_accumulator_at_every_span() {
    // Monotonicity is the property an overflowing numerator broke: the same
    // step read a *lower* alpha further into its period. Checked across the
    // whole range the type admits, both ends included.
    for nanos in [1, 1_000, 1_000_000, 66_666_666, 1_000_000_000, u32::MAX] {
        let span = span(nanos);
        let mut last = Factor16::from_bits(0);
        for numerator in 0..=8_u64 {
            let mut step = Step::new(span);
            // A fraction of the period, computed in integers like everything
            // else here.
            let into = u64::from(nanos) / 9 * numerator;
            step.advance(Duration::from_nanos(into));

            let alpha = step.alpha();
            assert!(
                alpha.to_bits() >= last.to_bits(),
                "span {nanos}: alpha fell from {} to {} at {into} ns in",
                last.to_bits(),
                alpha.to_bits(),
            );
            assert!(
                alpha.to_bits() <= Factor16::ONE.to_bits(),
                "span {nanos}: alpha {} is past one",
                alpha.to_bits(),
            );
            last = alpha;
        }
    }
}

#[test]
fn an_absurd_elapsed_against_the_longest_span_neither_panics_nor_wraps() {
    // The accumulator is a `u64` and saturates, so the worst input is one that
    // fills it against the widest divisor. Both halves of the arithmetic have
    // to survive that, not only the interpolation.
    let mut step = Step::new(TickSpan::MAX).with_catchup(u32::MAX);

    step.advance(Duration::MAX);
    assert!(step.alpha().to_bits() <= Factor16::ONE.to_bits());

    // And the step is still usable afterwards, which is what says the
    // saturation left the accumulator somewhere sensible rather than wrapped.
    let mut step = Step::new(TickSpan::MAX);
    assert_eq!(step.advance(TickSpan::MAX.period()), 1);
    assert_eq!(step.alpha(), Factor16::from_bits(0));
    assert_eq!(step.dropped(), 0);
}
