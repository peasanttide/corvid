//! Spans no tick rate names, which is where the arithmetic used to run out.
//!
//! `TickSpan::from_hz` cannot produce a span longer than a second, so every
//! other test in this crate stays inside one. Nothing holds a span there:
//! `from_nanos` takes any `NonZeroU64`, `From<NonZeroU64>` does too, and under
//! the `serde` feature a span is `transparent` over that integer — so a save
//! file or a peer can hand this crate any span at all.
//!
//! Past about thirty-nine hours, `alpha`'s numerator left a `u64`. That failed
//! two different ways in the two profiles this workspace ships: a panic where
//! overflow checks are on, and a wrong factor where they are off — and the
//! wrong factor was not even monotonic, so a renderer reading it saw the
//! picture jump backwards as the span grew. Neither profile's test suite could
//! see it, because neither had a span this long in it.
//!
//! A turn-based game, a persistent world ticking once a minute, and a headless
//! server catching up a day of simulation are all inside this range and none of
//! them is exotic.

use core::num::NonZeroU64;
use core::time::Duration;

use corvid_fixed::Factor16;
use corvid_time::{Step, TickSpan};

/// A span from a plain number of nanoseconds, which is the constructor with no
/// ceiling under it. Spelled without an `unwrap` so it works in a `const`, as
/// `tests/step.rs` spells its rates.
const fn span(nanos: u64) -> TickSpan {
    match NonZeroU64::new(nanos) {
        Some(nanos) => TickSpan::from_nanos(nanos),
        None => TickSpan::CRADLE,
    }
}

/// Just past where `accumulated_nanos * 65535` used to leave a `u64`.
const PAST_THE_CLIFF: u64 = 140_739_635_871_745;

#[test]
fn a_span_longer_than_a_day_still_answers_an_alpha() {
    let day = span(24 * 60 * 60 * 1_000_000_000);
    let mut step = Step::new(day);

    // Six hours into a day is a quarter of the way to the next tick.
    step.advance(Duration::from_secs(6 * 60 * 60));
    assert_eq!(step.alpha(), Factor16::from_bits(16384));

    // And eighteen hours is three quarters, which is the half of this that a
    // wrapped multiplication got wrong rather than merely imprecise.
    step.advance(Duration::from_secs(12 * 60 * 60));
    assert_eq!(step.alpha(), Factor16::from_bits(49151));
}

#[test]
fn alpha_climbs_with_the_accumulator_at_every_span() {
    // Monotonicity is the property a wrapped numerator broke: the same step
    // read a *lower* alpha further into its period. Checked across the cliff
    // rather than near it, because the failure was a wrap and not a rounding.
    for nanos in [
        1,
        1_000_000_000,
        PAST_THE_CLIFF - 1,
        PAST_THE_CLIFF,
        u64::MAX / 2,
        u64::MAX,
    ] {
        let span = span(nanos);
        let mut last = Factor16::from_bits(0);
        for numerator in 0..=8_u64 {
            let mut step = Step::new(span);
            // A fraction of the period, computed in integers like everything
            // else here.
            let into = nanos / 9 * numerator;
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
fn the_longest_span_there_is_neither_panics_nor_wraps() {
    // The value a `serde(transparent)` span deserializes from `u64::MAX`, which
    // is the worst input reachable without a single line of caller error.
    let mut step = Step::new(span(u64::MAX));

    // Nothing advanced yet, so nothing of the period has elapsed.
    assert_eq!(step.alpha(), Factor16::from_bits(0));

    // And an ordinary frame against a span that long is still nothing of it,
    // rather than the `from_bits(1)` a release build used to answer.
    step.advance(Duration::from_millis(16));
    assert_eq!(step.alpha(), Factor16::from_bits(0));

    // No tick is owed either, because a frame is not a period.
    assert_eq!(step.dropped(), 0);
}

#[test]
fn a_span_that_long_still_delivers_its_tick_when_the_period_is_up() {
    // The other side of the same arithmetic: the division that yields whole
    // ticks has to survive the widening too.
    let span = span(PAST_THE_CLIFF);
    let mut step = Step::new(span);

    assert_eq!(step.advance(span.period()), 1);
    assert_eq!(step.alpha(), Factor16::from_bits(0));
    assert_eq!(step.dropped(), 0);
}
