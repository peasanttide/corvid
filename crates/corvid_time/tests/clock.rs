//! The clock, which the simulation never sees.
//!
//! `Clock` exists so the loop that drives a game can be handed `Fake` in a test
//! and `Wall` in production. What that buys is that a headless run finishes as
//! fast as the processor can manage instead of in real time, and that a test
//! about the hundredth tick is not also a test about the machine it runs on.

use core::marker::PhantomData;
use core::time::Duration;

use corvid_time::{Clock, Fake, Step, Tick, TickSpan};

/// Asks whether `T` is `Copy` without requiring that it is.
///
/// `Probe::<T>::IS_COPY` resolves to the inherent constant when the `T: Copy`
/// bound holds and falls back to the trait's default when it does not, so the
/// answer is a `bool` a test can assert on rather than a compile error. There
/// is no other way to state "this type is deliberately not `Copy`" as a test:
/// the code that would break if `Copy` came back is code that does not compile
/// today, so it cannot be written down here.
struct Probe<T>(PhantomData<T>);

/// The fallback answer, used for every type that does not satisfy `T: Copy`.
trait MaybeCopy {
    /// Whether the probed type is `Copy`.
    const IS_COPY: bool = false;
}

impl<T> MaybeCopy for Probe<T> {}

impl<T: Copy> Probe<T> {
    /// The answer for types that are `Copy`, which shadows the trait's default.
    const IS_COPY: bool = true;
}

#[test]
fn a_clock_is_not_copy_because_reading_one_consumes_it() {
    // A `Copy` clock hands the same queued time to every copy of itself, so a
    // frame time passed by value to a helper is a frame time the caller also
    // still holds — two ticks' worth of one tick. `Step` is not `Copy` for
    // exactly this reason and a clock is the same kind of object.
    //
    // These are `const` blocks because the answer is known at compile time and
    // a runtime assertion on a constant is a lint. It means a clock that grew a
    // `Copy` back fails the build rather than the run, which is if anything the
    // louder of the two.
    const { assert!(!Probe::<Fake>::IS_COPY, "Fake is Copy; see Step's own note") }
    #[cfg(feature = "std")]
    const {
        assert!(
            !Probe::<corvid_time::Wall>::IS_COPY,
            "Wall is Copy; see Step's own note"
        );
    }
    // Not a `Copy` bound in disguise: the probe answers honestly for a type
    // that really is one.
    const { assert!(Probe::<Duration>::IS_COPY) }
}

#[test]
fn cloning_a_clock_duplicates_the_time_it_is_holding() {
    // `Clone` is kept, so this is not a bug — it is the hazard `Copy` would
    // have made implicit, written out. Both halves hand back the same thirty
    // milliseconds, and the `clone` is where a reader can see why.
    let mut clock = Fake::new();
    clock.advance(Duration::from_millis(30));
    let mut forked = clock.clone();
    assert_eq!(clock.elapsed(), Duration::from_millis(30));
    assert_eq!(forked.elapsed(), Duration::from_millis(30));
    assert_eq!(clock.elapsed(), Duration::ZERO);
}

#[test]
fn a_fresh_fake_has_no_time_in_it() {
    let mut clock = Fake::new();
    assert_eq!(clock.elapsed(), Duration::ZERO);
    assert_eq!(Fake::default(), Fake::new());
}

#[test]
fn a_fake_hands_back_what_was_queued_once() {
    let mut clock = Fake::new();
    clock.advance(Duration::from_millis(30));
    clock.advance(Duration::from_millis(70));
    assert_eq!(clock.elapsed(), Duration::from_millis(100));
    assert_eq!(clock.elapsed(), Duration::ZERO);
}

#[test]
fn a_stepping_fake_returns_exactly_one_period_per_call() {
    let period = TickSpan::CRADLE.period();
    let mut clock = Fake::stepping(period);
    for _ in 0..1000 {
        assert_eq!(clock.elapsed(), period);
    }
}

#[test]
fn a_stepping_fake_drives_exactly_one_tick_per_call() {
    // This is the shape of every headless test in the workspace: no stall is
    // possible, so no tick is ever dropped, and the thousandth call is the
    // thousandth tick.
    let rate = TickSpan::CRADLE;
    let mut clock = Fake::stepping(rate.period());
    let mut step = Step::new(rate);
    let mut tick = Tick::ZERO;
    for _ in 0..1000 {
        assert_eq!(step.advance(clock.elapsed()), 1);
        tick = tick.next();
    }
    assert_eq!(tick, Tick(1000));
    assert_eq!(step.dropped(), 0);
}

#[test]
fn a_stepping_fake_still_takes_a_nudge() {
    let mut clock = Fake::stepping(Duration::from_millis(10));
    clock.advance(Duration::from_millis(5));
    assert_eq!(clock.elapsed(), Duration::from_millis(15));
    assert_eq!(clock.elapsed(), Duration::from_millis(10));
}

#[test]
fn a_clock_can_be_held_as_a_trait_object() {
    // The runtime stores whichever clock it was built with, so the trait has to
    // stay object safe.
    let mut clock: Box<dyn Clock> = Box::new(Fake::stepping(Duration::from_millis(1)));
    assert_eq!(clock.elapsed(), Duration::from_millis(1));
}

#[cfg(feature = "std")]
#[test]
fn a_wall_clock_measures_forward_only() {
    use corvid_time::Wall;

    let mut clock = Wall::new();
    let first = clock.elapsed();
    let mut total = Duration::ZERO;
    for _ in 0..100 {
        total = total.saturating_add(clock.elapsed());
    }
    // Nothing here asserts on how long anything took — only that a wall clock
    // never hands back a negative interval and never panics doing it.
    assert!(first < Duration::from_mins(1));
    assert!(total < Duration::from_mins(1));
}

#[cfg(feature = "std")]
#[test]
fn a_wall_clock_measures_a_sleep() {
    use corvid_time::Wall;

    let mut clock = Wall::new();
    clock.elapsed();
    std::thread::sleep(Duration::from_millis(20));
    assert!(clock.elapsed() >= Duration::from_millis(15));
}

#[cfg(feature = "std")]
#[test]
fn a_wall_clock_reports_the_interval_since_the_last_call_not_since_it_was_built() {
    use corvid_time::Wall;

    // This is the contract the whole trait is written around, and measuring a
    // single sleep does not test it: a clock that forgot to move its mark
    // forward would return an ever-growing time-since-construction and pass
    // that just as well. What separates the two is the reading *after* the
    // sleep, with nothing in between for it to have measured.
    //
    // `Wall` is the only clock a game actually runs on, and the loop feeds
    // whatever it says straight into `Step::advance`. A timestamp there means
    // every frame after the first hands the step a growing interval, so the
    // catch-up ceiling refuses ticks forever and the simulation never recovers.
    let mut clock = Wall::new();
    clock.elapsed();
    std::thread::sleep(Duration::from_millis(20));
    let slept = clock.elapsed();
    let immediately_after = clock.elapsed();

    assert!(slept >= Duration::from_millis(15), "measured {slept:?}");
    // A generous ceiling, because this runs on shared CI hardware where a
    // thread can lose the processor for a while between two adjacent lines.
    // Even so it is an order of magnitude under the sleep it must not repeat.
    assert!(
        immediately_after < Duration::from_millis(10),
        "two consecutive reads with nothing between them measured \
         {immediately_after:?}; the clock is reporting a timestamp, not an interval"
    );

    // And it keeps doing so: a hundred back-to-back reads of an interval clock
    // sum to about the time the loop took, not to a hundred times the sleep.
    let mut total = Duration::ZERO;
    for _ in 0..100 {
        total = total.saturating_add(clock.elapsed());
    }
    assert!(
        total < Duration::from_millis(100),
        "a hundred immediate reads summed to {total:?}"
    );
}

#[cfg(feature = "std")]
#[test]
fn a_wall_clock_never_hands_a_step_more_time_than_actually_passed() {
    use core::num::NonZeroU32;

    use corvid_time::Wall;

    // The same contract stated the way the loop writes it: however long this
    // machine really took, the ticks the step delivers cannot exceed the
    // periods that fit in it, and cannot fall far short of the ones the sleep
    // alone guarantees. A clock reporting timestamps instead of intervals
    // re-hands the sleep on every iteration, so twenty frames owe twenty
    // sleeps' worth of ticks out of one sleep's worth of time.
    //
    // The rate is a kilohertz and the sleep is fifty milliseconds, so fifty
    // whole periods fit inside the interval being measured. That ratio is the
    // point of the numbers: a clock that inflated every interval even slightly
    // would deliver more ticks than the elapsed time affords, where the cradle
    // rate's sixty-six millisecond period against a twenty millisecond sleep
    // left room for a clock to multiply what it reported several times over
    // and still round down to the same tick count.
    const KILOHERTZ: TickSpan = match NonZeroU32::new(1_000) {
        Some(hz) => TickSpan::from_hz(hz),
        None => TickSpan::CRADLE,
    };

    let rate = KILOHERTZ;
    let mut clock = Wall::new();
    // The ceiling must not be what bounds this test — the elapsed time must
    // be — so it is set where it can never bind, even if this thread loses the
    // processor for seconds between the sleep and the reads.
    let mut step = Step::new(rate).with_catchup(u32::MAX);
    let started = std::time::Instant::now();

    clock.elapsed();
    std::thread::sleep(Duration::from_millis(50));
    let mut ticks = 0u64;
    for _ in 0..20 {
        ticks += u64::from(step.advance(clock.elapsed()));
    }
    let truth = started.elapsed();

    // The accumulator started empty and every interval the clock reported lies
    // between the first read and the last, both of which happen inside
    // `truth`. So the step was told about at most `truth` nanoseconds and can
    // have delivered at most the whole periods that fit in them — no slack is
    // owed, and none is given.
    let affordable = u64::try_from(truth.as_nanos() / u128::from(rate.nanos())).unwrap_or(u64::MAX);
    assert!(
        ticks <= affordable,
        "the step delivered {ticks} ticks out of {truth:?}, which affords {affordable}"
    );

    // The other side, because a bound in one direction is satisfied by a clock
    // that reports nothing at all. Two bounds, because neither alone catches a
    // clock that under-reports by a few percent.
    //
    // The first is exact rather than slack. `sleep` never returns early, so the
    // measured window contains fifty whole periods, the accumulator started
    // empty, and the ceiling cannot bind — so fifty ticks were owed and fifty
    // were delivered. There is no reason to ask for less, and asking for less
    // is what let an `elapsed` scaled by nine tenths through: forty-five is
    // exactly what such a clock reports.
    assert!(
        ticks >= 50,
        "a fifty millisecond sleep yielded {ticks} ticks of a one millisecond period"
    );

    // The first bound loosens on a loaded machine, where the sleep overshoots
    // and the extra periods pay for the shortfall — with the processor
    // oversubscribed four to one this loop has been seen to deliver sixty-six
    // ticks, and a clock reporting nineteen twentieths of sixty-six still
    // clears fifty. So the second bound is relative, comparing what the step
    // was told against what an independent `Instant` says really passed.
    //
    // One period of slack and no more. The reported window sits strictly inside
    // `truth` — it starts one `Instant::now()` later and ends one earlier — so
    // the two can straddle a period boundary and differ by one for that reason
    // alone. They cannot differ by two without a whole millisecond vanishing
    // into a gap that is a hundred nanoseconds wide. Over eight hundred runs,
    // four hundred of them with ninety-six spinners on twenty-four cores, the
    // difference measured zero every time; an `elapsed` scaled by nineteen
    // twentieths measures two or three.
    assert!(
        ticks + 1 >= affordable,
        "the step was told about {ticks} periods of the {affordable} that really passed"
    );
}
