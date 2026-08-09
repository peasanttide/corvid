//! The fixed step: how real time is turned into a whole number of ticks.
//!
//! Two properties carry this file. Advancing by exactly one period, a thousand
//! times, must produce exactly a thousand ticks -- an accumulator that rounded
//! anywhere would lose or gain one and a replay would stop matching. And a
//! process that stalls must *drop* the ticks it could not deliver rather than
//! bank them -- `Step::advance` is where that argument is written down, and what
//! is checked here is that the code does it.

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
fn exact_multiples_do_not_drift() {
    let mut step = Step::new(TickSpan::CRADLE);
    let mut total = 0;
    for _ in 0..1000 {
        total += step.advance(Duration::from_nanos(66_666_666));
    }
    // Exactly, and the exactness is the whole claim: a tolerance of one here
    // would pass for the off-by-one accumulator this test is named after, which
    // is a replay one tick short of the run it recorded.
    assert_eq!(total, 1000, "drifted to {total}");
}

#[test]
fn a_thousand_periods_are_a_thousand_ticks() {
    for hz in [10, 15, 20, 30, 60, 64, 144] {
        let rate = rate(hz);
        let mut step = Step::new(rate).with_catchup(1);
        let mut total = 0;
        for _ in 0..1000 {
            total += step.advance(rate.period());
        }
        assert_eq!(
            total, 1000,
            "{hz} Hz delivered {total} ticks in 1000 periods"
        );
        assert_eq!(
            step.dropped(),
            0,
            "{hz} Hz dropped a tick it was handed exactly"
        );
        assert_eq!(step.alpha(), Factor16::ZERO, "{hz} Hz ended mid-period");
    }
}

#[test]
fn a_period_split_into_pieces_is_still_one_tick() {
    // A hundred advances of a hundredth of the period must produce one tick,
    // not zero: the remainder has to survive from one call to the next.
    let mut step = Step::new(rate(10));
    let mut total = 0;
    for _ in 0..100 {
        total += step.advance(Duration::from_millis(1));
    }
    assert_eq!(total, 1);
    assert_eq!(step.alpha(), Factor16::ZERO);
}

#[test]
fn ragged_advances_deliver_the_whole_elapsed_time() {
    // Frame times in the wild are never a multiple of the period. Over a run,
    // the ticks delivered must still account for every nanosecond handed in,
    // to within the part of a period left over at the end.
    let rate = TickSpan::CRADLE;
    let mut step = Step::new(rate).with_catchup(64);
    let mut elapsed = 0u64;
    let mut total = 0u64;
    for frame in 0..10_000u64 {
        let nanos = 7_000_000 + (frame * 2_654_435_761) % 23_000_000;
        elapsed += nanos;
        total += u64::from(step.advance(Duration::from_nanos(nanos)));
    }
    assert_eq!(total, elapsed / u64::from(rate.nanos()));
    assert_eq!(step.dropped(), 0);
}

#[test]
fn a_stalled_process_drops_ticks_rather_than_banking_them() {
    let mut step = Step::new(TickSpan::CRADLE).with_catchup(4);
    assert_eq!(step.advance(Duration::from_secs(10)), 4);
    assert_eq!(step.advance(Duration::from_millis(1)), 0);
    // Exactly 146: ten seconds is 150 periods at fifteen hertz and four were
    // delivered. A `>=` would have passed for a step that dropped the four it
    // handed back as well.
    assert_eq!(step.dropped(), 146);
}

#[test]
fn the_second_after_a_stall_is_an_ordinary_second() {
    // What dropping is for, checked rather than argued: the fifteen periods
    // after a ten-second stall deliver fifteen ticks and nothing more.
    let rate = TickSpan::CRADLE;
    let mut step = Step::new(rate).with_catchup(4);
    step.advance(Duration::from_secs(10));

    let mut total = 0;
    for _ in 0..15 {
        total += step.advance(rate.period());
    }
    assert_eq!(total, 15);
}

#[test]
fn the_remainder_below_a_period_survives_a_stall() {
    // Dropping is about whole ticks and only whole ticks. The sub-tick
    // remainder is not part of the backlog, so a stall must leave it exactly
    // where it was -- that is what puts the ticks that *are* delivered on the
    // schedule they would have had without the stall, and what lets alpha pick
    // up where it left off instead of snapping to zero.
    //
    // Ten hertz is a period of 100 ms exactly, so a quarter of the way in is
    // 16 384 of the factor's 65 535 and can be written down by hand.
    let rate = rate(10);
    let mut step = Step::new(rate).with_catchup(2);
    assert_eq!(step.advance(Duration::from_millis(25)), 0);
    assert_eq!(step.alpha(), Factor16::from_bits(16384));

    // Ten seconds is a hundred whole periods and not a nanosecond more, so the
    // 25 ms is untouched by the stall and untouched by the ceiling refusing 98
    // of the hundred ticks it owed.
    assert_eq!(step.advance(Duration::from_secs(10)), 2);
    assert_eq!(step.dropped(), 98);
    assert_eq!(
        step.alpha(),
        Factor16::from_bits(16384),
        "the stall swallowed the sub-tick remainder"
    );

    // And the remainder is not merely reported: the next 75 ms completes the
    // period it was a quarter of, which is one tick rather than none.
    assert_eq!(
        step.advance(Duration::from_millis(75)),
        1,
        "the tick the surviving remainder was three-quarters of the way to"
    );
    assert_eq!(step.alpha(), Factor16::ZERO);
}

#[test]
fn the_catchup_ceiling_is_what_bounds_one_advance() {
    for ceiling in [1u32, 2, 8, 64] {
        let mut step = Step::new(TickSpan::CRADLE).with_catchup(ceiling);
        assert_eq!(step.advance(Duration::from_mins(1)), ceiling);
    }
}

#[test]
fn a_catchup_of_zero_would_stop_the_simulation_so_it_is_raised_to_one() {
    let mut step = Step::new(TickSpan::CRADLE).with_catchup(0);
    assert_eq!(step.catchup(), 1);
    assert_eq!(step.advance(Duration::from_secs(1)), 1);
}

#[test]
fn dropped_counts_every_tick_the_ceiling_refused() {
    let mut step = Step::new(rate(10)).with_catchup(2);
    assert_eq!(step.advance(Duration::from_secs(1)), 2);
    assert_eq!(step.dropped(), 8);
    assert_eq!(step.advance(Duration::from_secs(1)), 2);
    assert_eq!(step.dropped(), 16);
}

#[test]
fn keeping_up_drops_nothing() {
    let rate = TickSpan::CRADLE;
    let mut step = Step::new(rate);
    for _ in 0..1000 {
        step.advance(rate.period());
    }
    assert_eq!(step.dropped(), 0);
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

#[test]
fn a_stalled_advance_cannot_overflow_the_accumulator() {
    // `Duration` counts to 584 billion years and the accumulator counts to 584
    // of them, so the conversion has to saturate rather than wrap -- a wrapped
    // accumulator would hand back a tick count from the far side of the wrap.
    //
    // `Duration::MAX` is the wrong input to prove that with. Its nanosecond
    // count is (2^64 - 1) * 10^9 + 999_999_999, which is congruent to 2^64 - 1
    // modulo 2^64, so a truncating `as u64` lands on `u64::MAX` there too and
    // the two implementations are indistinguishable. The first input below is
    // exactly 2^64 nanoseconds, which truncates to zero and no ticks.
    const TWO_POW_64_NANOS: Duration = Duration::new(18_446_744_073, 709_551_616);

    let mut step = Step::new(TickSpan::CRADLE).with_catchup(3);
    assert_eq!(
        step.advance(TWO_POW_64_NANOS),
        3,
        "2^64 ns wrapped to a small number of nanoseconds"
    );
    // Saturating to `u64::MAX` rather than to some other large number is
    // visible here and nowhere else: 2^64 - 1 ns is 276 701 163 872 whole
    // cradle periods, and a ceiling of three refused all but three of them.
    assert_eq!(
        step.dropped(),
        276_701_163_869,
        "the conversion saturated somewhere other than the top of a u64"
    );

    // One nanosecond over a multiple of 2^64 wraps to one nanosecond, which is
    // below a period and so is also zero ticks -- but it leaves a residue behind
    // that a later advance would then be short by.
    let mut step = Step::new(TickSpan::CRADLE).with_catchup(3);
    assert_eq!(
        step.advance(TWO_POW_64_NANOS.saturating_add(Duration::from_nanos(1))),
        3
    );

    // Saturating leaves a particular remainder rather than whatever fell out.
    // The first advance fills the accumulator to 2^64 - 1 ns exactly and the
    // second saturates it back there, and 2^64 - 1 ns is 276 701 163 872 whole
    // cradle periods with 43 660 863 ns left over. Scaled onto the factor's
    // 65 535 steps and rounded, 43 660 863 * 65 535 / 66 666 666 is 42 920.
    let rate = TickSpan::CRADLE;
    let mut step = Step::new(rate).with_catchup(3);
    assert_eq!(step.advance(Duration::MAX), 3);
    assert_eq!(step.advance(Duration::MAX), 3);
    assert_eq!(step.alpha(), Factor16::from_bits(42_920));

    // And the remainder is live rather than merely reported: one further period
    // completes into exactly one tick and leaves the same 43 660 863 ns behind
    // it. An accumulator left at or above a period would deliver more than one
    // tick here, and the readings either side pin the remainder itself.
    assert_eq!(step.advance(rate.period()), 1);
    assert_eq!(step.alpha(), Factor16::from_bits(42_920));
}

#[test]
fn the_dropped_counter_saturates_rather_than_wrapping() {
    // `dropped` is a running total that nothing ever resets, so the only
    // honest answer at the top of its range is to stay there. Wrapping would
    // report a handful of dropped ticks for a process that had dropped every
    // tick there is, which is the one reading that would be acted on and the
    // one reading that would be wrong.
    //
    // Reaching the top takes a rate whose period is a nanosecond -- above a
    // gigahertz the period clamps there -- so that one maximal advance owes
    // `u64::MAX` ticks and a ceiling of one refuses all but one of them. Two
    // advances then more than cover the range.
    let mut step = Step::new(rate(u32::MAX)).with_catchup(1);
    assert_eq!(step.span().nanos(), 1, "the period did not clamp");

    assert_eq!(step.advance(Duration::MAX), 1);
    assert_eq!(step.dropped(), u64::MAX - 1);
    assert_eq!(step.advance(Duration::MAX), 1);
    assert_eq!(step.dropped(), u64::MAX);

    // And it stays at the top rather than rolling over on the advance after.
    assert_eq!(step.advance(Duration::MAX), 1);
    assert_eq!(step.dropped(), u64::MAX);
}

#[test]
fn zero_elapsed_is_zero_ticks() {
    let mut step = Step::new(TickSpan::CRADLE);
    assert_eq!(step.advance(Duration::ZERO), 0);
    assert_eq!(step.alpha(), Factor16::ZERO);
    assert_eq!(step.dropped(), 0);
}

#[test]
fn a_step_remembers_the_rate_it_was_built_from() {
    let step = Step::new(rate(144));
    assert_eq!(step.span(), rate(144));
    assert_eq!(step.span().nanos(), 6_944_444);
}

/// Nothing in `src/` may compute on a floating-point value, `alpha` least of
/// all -- `Step::alpha` is where that is argued. This test reads the crate's own
/// source rather than its behaviour, because a division that rounds the same
/// way on this machine is not evidence of anything.
#[test]
fn no_floating_point_anywhere_in_the_crate() {
    // Every module in `src/`. A hand-written list is the failure mode this test
    // has by construction -- it is the one test whose job is to be exhaustive,
    // and a module nobody added to the list is a module it silently skips --
    // so the count is asserted against the directory rather than trusted.
    const SOURCES: [(&str, &str); 6] = [
        ("src/lib.rs", include_str!("../src/lib.rs")),
        ("src/tick.rs", include_str!("../src/tick.rs")),
        ("src/ticks.rs", include_str!("../src/ticks.rs")),
        ("src/span.rs", include_str!("../src/span.rs")),
        ("src/step.rs", include_str!("../src/step.rs")),
        ("src/clock.rs", include_str!("../src/clock.rs")),
    ];

    // `into_iter().flatten()` rather than an `unwrap`, which this file spells
    // nowhere: a directory that could not be read counts zero modules and fails
    // the assertion below, which is the right answer either way.
    let modules = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))
        .into_iter()
        .flatten()
        .filter(|entry| {
            entry
                .as_ref()
                .is_ok_and(|entry| entry.path().extension().is_some_and(|ext| ext == "rs"))
        })
        .count();
    assert_eq!(
        modules,
        SOURCES.len(),
        "src/ has {modules} modules and this test lists {}",
        SOURCES.len(),
    );

    for (name, source) in SOURCES {
        for forbidden in ["f32", "f64"] {
            assert!(
                !source.contains(forbidden),
                "{name} names `{forbidden}`; simulation time is integer arithmetic"
            );
        }

        // Naming a floating-point type is the obvious way in; inferring one
        // from a literal is the quiet way, and `0.5` is exactly the literal
        // somebody computing a fraction would reach for.
        let bytes = source.as_bytes();
        for window in bytes.windows(3) {
            assert!(
                !(window[0].is_ascii_digit() && window[1] == b'.' && window[2].is_ascii_digit()),
                "{name} carries a decimal literal; simulation time is integer arithmetic"
            );
        }
    }
}
