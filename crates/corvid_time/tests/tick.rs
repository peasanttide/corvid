//! The tick number and the rate it is counted at.
//!
//! Both are simulation state — a tick goes in a save, a rate goes in an opening
//! — so both have to be total, ordered, and encoded the same way on every
//! machine. Nothing here may panic, because a tick arriving from a save file or
//! a peer is untrusted input and arithmetic on it happens inside `tick`.

use core::num::NonZeroU32;
use core::time::Duration;

use corvid_time::{Tick, TickSpan};

const fn rate(hz: u32) -> TickSpan {
    match NonZeroU32::new(hz) {
        Some(hz) => TickSpan::from_hz(hz),
        None => TickSpan::CRADLE,
    }
}

#[test]
fn a_tick_counts_from_zero() {
    assert_eq!(Tick::ZERO, Tick(0));
    assert_eq!(Tick::default(), Tick::ZERO);
    assert_eq!(Tick::ZERO.next(), Tick(1));
    assert_eq!(Tick(1).prev(), Tick::ZERO);
}

#[test]
fn the_ends_saturate_rather_than_wrapping() {
    assert_eq!(Tick::ZERO.prev(), Tick::ZERO);
    assert_eq!(Tick(u64::MAX).next(), Tick(u64::MAX));
    assert_eq!(Tick::ZERO.saturating_sub(9), Tick::ZERO);
    assert_eq!(Tick(u64::MAX).saturating_add(9), Tick(u64::MAX));
}

#[test]
fn since_measures_forward_and_is_zero_backward() {
    assert_eq!(Tick(100).since(Tick(60)), 40);
    assert_eq!(Tick(60).since(Tick(100)), 0);
    assert_eq!(Tick(60).since(Tick(60)), 0);
}

#[test]
fn ticks_order_by_number() {
    assert!(Tick(1) < Tick(2));
    let mut ticks = [Tick(3), Tick(1), Tick(2)];
    ticks.sort_unstable();
    assert_eq!(ticks, [Tick(1), Tick(2), Tick(3)]);
}

#[test]
fn a_tick_displays_as_its_number() {
    assert_eq!(Tick(4127).to_string(), "4127");
}

#[test]
fn the_cradle_rate_is_fifteen_hertz() {
    assert_eq!(TickSpan::CRADLE.hz(), 15);
    assert_eq!(TickSpan::default(), TickSpan::CRADLE);
    assert_eq!(TickSpan::CRADLE.period(), Duration::from_nanos(66_666_666));
}

#[test]
fn a_period_is_a_whole_number_of_nanoseconds() {
    for hz in [1u32, 10, 15, 20, 24, 30, 50, 60, 64, 90, 120, 144, 240] {
        let rate = rate(hz);
        assert_eq!(rate.period(), Duration::from_nanos(rate.nanos()));
        assert_eq!(rate.nanos(), 1_000_000_000 / u64::from(hz));
    }
}

#[test]
fn the_residual_of_a_period_is_one_nanosecond_per_second_per_leftover() {
    // Truncating the period makes the simulation run fast by exactly
    // `1_000_000_000 % hz` nanoseconds per second of real time. Writing that
    // down as a test is what keeps the README's table honest.
    for hz in [10u32, 15, 30, 60, 64, 144] {
        let rate = rate(hz);
        let per_second = 1_000_000_000 - u64::from(hz) * rate.nanos();
        assert_eq!(per_second, u64::from(1_000_000_000 % hz));
    }
}

#[test]
fn an_absurd_rate_still_has_a_period() {
    // Above a gigahertz the truncated period would be zero, and a zero period
    // is a division by zero in the step. It clamps to one nanosecond instead.
    assert_eq!(rate(u32::MAX).nanos(), 1);
    assert_eq!(rate(2_000_000_000).period(), Duration::from_nanos(1));
    assert_eq!(rate(1).period(), Duration::from_secs(1));
}

#[cfg(feature = "serde")]
#[test]
fn a_tick_is_a_number_on_the_wire() {
    assert_eq!(
        serde_json::to_string(&Tick(4127)).ok(),
        Some("4127".to_owned())
    );
    assert_eq!(serde_json::from_str::<Tick>("4127").ok(), Some(Tick(4127)));
}

#[cfg(feature = "serde")]
#[test]
fn a_span_is_a_number_of_nanoseconds_on_the_wire_and_zero_is_refused() {
    // Nanoseconds, not hertz. In 0.1 this read `15`.
    assert_eq!(
        serde_json::to_string(&TickSpan::CRADLE).ok(),
        Some("66666666".to_owned())
    );
    assert_eq!(
        serde_json::from_str::<TickSpan>("66666666").ok(),
        Some(TickSpan::CRADLE)
    );
    assert!(serde_json::from_str::<TickSpan>("0").is_err());
}

#[test]
fn adjacent_ticks_digest_differently() {
    use corvid_hash::digest;

    let mut seen = std::collections::HashSet::new();
    for number in 0..10_000u64 {
        assert!(
            seen.insert(digest(&Tick(number))),
            "collision at tick {number}"
        );
    }
}

#[test]
fn a_span_digests_as_its_nanoseconds() {
    use corvid_hash::digest;

    // The nanoseconds and nothing else, at the width the field is stored at —
    // a `u64` since 0.2, where this was a `u32` of hertz.
    assert_eq!(digest(&TickSpan::CRADLE), digest(&66_666_666u64));
    assert_ne!(digest(&TickSpan::CRADLE), digest(&rate(30)));

    // Two rates that truncate to the same span are the same simulation and
    // digest alike, which they could not do while hertz was what was stored.
    // 62 500 001 through 66 666 666 nanoseconds are all "fifteen hertz"; these
    // two rates land on the same span and so on the same digest.
    assert_eq!(rate(15), rate(15));
    assert_ne!(digest(&rate(15)), digest(&rate(16)));
}
