//! Alpha: where the clock stands between two ticks, as a fixed-point factor.
//!
//! A renderer draws between ticks and needs to know how far between, and the
//! answer has to come out of integer arithmetic like everything else here. What
//! is checked is that alpha is the exact ratio the accumulator says it is, that
//! it climbs inside a period and lands on exactly zero when the tick comes due,
//! and that it agrees with an accumulator this file keeps for itself across
//! frames that do not divide the period.

use core::num::NonZeroU32;
use core::time::Duration;

use corvid_fixed::Factor16;
use corvid_time::{Step, TickSpan};

/// A rate in hertz, spelled without an `unwrap` so it works in a `const`. Zero
/// is not a rate and falls back to [`TickSpan::CRADLE`].
const fn rate(hz: u32) -> TickSpan {
    match NonZeroU32::new(hz) {
        Some(hz) => TickSpan::from_hz(hz),
        None => TickSpan::CRADLE,
    }
}

#[test]
fn alpha_walks_from_zero_to_one_between_ticks() {
    let mut step = Step::new(rate(10));
    assert_eq!(step.advance(Duration::from_millis(50)), 0);
    assert_eq!(step.alpha(), Factor16::from_f64(0.5));
    assert_eq!(step.advance(Duration::from_millis(50)), 1);
    assert_eq!(step.alpha(), Factor16::ZERO);
}

#[test]
fn alpha_is_the_exact_ratio_of_two_integers() {
    // Ten Hz has a period of exactly 100 000 000 ns, so the expected bits are
    // `round(millis * 65535 / 100)` and can be written down by hand.
    for (millis, bits) in [
        (0u64, 0u16),
        (10, 6554),
        (25, 16384),
        (50, 32768),
        (99, 64880),
    ] {
        let mut step = Step::new(rate(10));
        assert_eq!(step.advance(Duration::from_millis(millis)), 0);
        assert_eq!(
            step.alpha(),
            Factor16::from_bits(bits),
            "alpha at {millis} ms of a 100 ms period"
        );
    }
}

#[test]
fn alpha_climbs_within_a_period_and_is_exactly_zero_on_the_boundary() {
    // `alpha() <= Factor16::ONE` says nothing: `ONE` is `u16::MAX`, so it holds
    // for every conceivable implementation, including one that returns the
    // wrong end of the interval. The content is in the two ends and the shape
    // between them -- alpha only ever moves forward inside a period, it reaches
    // the top just before the tick comes due, and the tick puts it back at
    // exactly zero rather than somewhere near it.
    let rate = rate(10);
    let mut step = Step::new(rate);
    assert_eq!(step.alpha(), Factor16::ZERO);

    let mut previous = Factor16::ZERO;
    for millisecond in 1..100u64 {
        assert_eq!(step.advance(Duration::from_millis(1)), 0);
        let alpha = step.alpha();
        assert!(
            alpha > previous,
            "alpha went from {previous:?} to {alpha:?} at {millisecond} ms of a 100 ms period"
        );
        previous = alpha;
    }

    // One nanosecond short of the tick, rounding has already carried alpha to
    // the top of its range. That alpha reaches `ONE` at all is the half of the
    // interval a `<= ONE` assertion can never see, and reaching it slightly
    // early -- within half a factor's step of the tick, 763 ns of this 100 ms
    // period -- is the documented cost of rounding onto sixteen bits.
    assert_eq!(
        step.advance(Duration::from_nanos(999_999)),
        0,
        "a period is 100 ms, so 99.999999 ms is still short of it"
    );
    assert_eq!(step.alpha(), Factor16::ONE);

    // And the nanosecond that completes the period returns it to exactly zero,
    // not to a factor's-worth of leftover.
    assert_eq!(step.advance(Duration::from_nanos(1)), 1);
    assert_eq!(step.alpha(), Factor16::ZERO);
    assert_eq!(step.dropped(), 0);
}

#[test]
fn alpha_tracks_an_independent_accumulator_across_ragged_frames() {
    // The cradle period is not a round number, so this crosses tick boundaries
    // at two thousand different offsets within a period rather than at one
    // convenient one. The expected answer is recomputed here from a `u64` this
    // file owns, which is what makes it a check on the step's bookkeeping and
    // not just on its arithmetic.
    let rate = TickSpan::CRADLE;
    let period = u64::from(rate.nanos());
    let scale = u64::from(Factor16::ONE.to_bits());
    let mut step = Step::new(rate).with_catchup(64);
    let mut expected = 0u64;

    for frame in 0..2_000u64 {
        let nanos = frame * 33_333;
        let total = expected + nanos;
        let owed = total / period;
        expected = total % period;

        // The expected alpha comes from what alpha *means* -- the nearest
        // sixteen-bit factor to the accumulator's share of a period, ties
        // upward -- and not from the expression the step evaluates to get
        // there. A whole part and a leftover compared against half a period is
        // a different route to that same rounding, so rewriting the step's
        // arithmetic leaves this passing while changing what it rounds to
        // (truncating instead, breaking ties downward, scaling by 65 536)
        // fails it.
        let share = expected * scale;
        let mut bits = share / period;
        if 2 * (share % period) >= period {
            bits += 1;
        }

        assert_eq!(
            u64::from(step.advance(Duration::from_nanos(nanos))),
            owed,
            "frame {frame} owed {owed} ticks"
        );
        assert_eq!(
            step.alpha().to_bits(),
            u16::try_from(bits).unwrap_or(u16::MAX),
            "alpha disagrees with an accumulator of {expected} ns on frame {frame}"
        );
    }
    assert_eq!(step.dropped(), 0);
}
