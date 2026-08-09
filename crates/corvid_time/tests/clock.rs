//! The clock, which the simulation never sees.
//!
//! `Clock` exists so the loop that drives a game can be handed a fixed step in
//! a test and the operating system's clock in production; the `Elapsed` trait's
//! own documentation is where what that buys is written down.

use core::marker::PhantomData;
use core::time::Duration;

use corvid_time::{Clock, Elapsed, Step, Tick, TickSpan};

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
    // `Clock`'s own documentation argues why; what is checked here is that the
    // derive still matches the argument.
    //
    // A `const` block because the answer is known at compile time and a runtime
    // assertion on a constant is a lint. It means a clock that grew a `Copy`
    // back fails the build rather than the run, which is if anything the louder
    // of the two. The wall mode carries an `Instant` -- a `Copy` field, which is
    // exactly how a `Copy` could be derived back onto this by accident.
    const {
        assert!(
            !Probe::<Clock>::IS_COPY,
            "Clock is Copy; see Step's own note"
        );
    }
    // Not a `Copy` bound in disguise: the probe answers honestly for a type
    // that really is one.
    const { assert!(Probe::<Duration>::IS_COPY) }
}

#[test]
fn cloning_a_clock_duplicates_the_time_it_is_holding() {
    // `Clone` is kept, so this is not a bug -- it is the hazard `Copy` would
    // have made implicit, written out. Both halves hand back the same thirty
    // milliseconds, and the `clone` is where a reader can see why.
    let mut clock = Clock::still();
    clock.advance(Duration::from_millis(30));
    let mut forked = clock.clone();
    assert_eq!(clock.elapsed(), Duration::from_millis(30));
    assert_eq!(forked.elapsed(), Duration::from_millis(30));
    assert_eq!(clock.elapsed(), Duration::ZERO);
}

#[test]
fn a_fresh_still_clock_has_no_time_in_it() {
    let mut clock = Clock::still();
    assert_eq!(clock.elapsed(), Duration::ZERO);
    assert_eq!(Clock::default(), Clock::still());
}

#[test]
fn a_still_clock_hands_back_what_was_queued_once() {
    let mut clock = Clock::still();
    clock.advance(Duration::from_millis(30));
    clock.advance(Duration::from_millis(70));
    assert_eq!(clock.elapsed(), Duration::from_millis(100));
    assert_eq!(clock.elapsed(), Duration::ZERO);
}

#[test]
fn a_stepping_clock_returns_exactly_one_period_per_call() {
    let period = TickSpan::CRADLE.period();
    let mut clock = Clock::stepping(period);
    for _ in 0..1000 {
        assert_eq!(clock.elapsed(), period);
    }
}

#[test]
fn a_stepping_clock_drives_exactly_one_tick_per_call() {
    // This is the shape a headless test wants: no stall is possible, so no tick
    // is ever dropped, and the thousandth call is the thousandth tick.
    let rate = TickSpan::CRADLE;
    let mut clock = Clock::stepping(rate.period());
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
fn a_stepping_clock_still_takes_a_nudge() {
    let mut clock = Clock::stepping(Duration::from_millis(10));
    clock.advance(Duration::from_millis(5));
    assert_eq!(clock.elapsed(), Duration::from_millis(15));
    assert_eq!(clock.elapsed(), Duration::from_millis(10));
}

#[test]
fn a_clock_can_be_held_as_a_trait_object() {
    // The loop stores whichever clock it was built with, so the trait has to
    // stay object safe.
    let mut clock: Box<dyn Elapsed> = Box::new(Clock::stepping(Duration::from_millis(1)));
    assert_eq!(clock.elapsed(), Duration::from_millis(1));
}

#[cfg(feature = "std")]
#[test]
fn a_wall_clock_measures_forward_only() {
    use corvid_time::Clock;

    // Bounded against a window measured here rather than against a fixed
    // deadline. A `Duration` is unsigned, so "never negative" is not observable;
    // what is observable is that consecutive readings *partition* the span they
    // were taken across, so their sum cannot exceed it however long this thread
    // loses the processor for. A clock reporting a timestamp, or the time since
    // its own construction, sums to a large multiple of the span instead.
    let outside = std::time::Instant::now();
    let mut clock = Clock::wall();
    let mut total = Duration::ZERO;
    for _ in 0..100 {
        total = total.saturating_add(clock.elapsed());
    }
    let span = outside.elapsed();

    assert!(
        total <= span,
        "a hundred readings summed to {total:?} across a window of {span:?}",
    );
}

#[cfg(feature = "std")]
#[test]
fn a_wall_clock_measures_a_sleep() {
    use corvid_time::Clock;

    let mut clock = Clock::wall();
    clock.elapsed();
    std::thread::sleep(Duration::from_millis(20));
    assert!(clock.elapsed() >= Duration::from_millis(15));
}

#[cfg(feature = "std")]
#[test]
fn a_wall_clock_reports_the_interval_since_the_last_call_not_since_it_was_built() {
    use corvid_time::Clock;

    // This is the contract the whole trait is written around, and measuring a
    // single sleep does not test it: a clock that forgot to move its mark
    // forward would return an ever-growing time-since-construction and pass
    // that just as well. What separates the two is the reading *after* the
    // sleep, with nothing in between for it to have measured.
    //
    // The wall clock is the only one a game actually runs on, and the loop feeds
    // whatever it says straight into `Step::advance`. A timestamp there means
    // every frame after the first hands the step a growing interval, so the
    // catch-up ceiling refuses ticks forever and the simulation never recovers.
    let mut clock = Clock::wall();
    clock.elapsed();
    std::thread::sleep(Duration::from_millis(20));
    let bracket = std::time::Instant::now();
    let slept = clock.elapsed();
    let immediately_after = clock.elapsed();
    // Opened before the reading that starts the interval and closed after the
    // one that ends it, so the clock's window is contained in this one whatever
    // the scheduler does between the lines.
    let between = bracket.elapsed();

    assert!(slept >= Duration::from_millis(15), "measured {slept:?}");
    // The second reading is bounded by a window measured around it rather than
    // by a deadline: whatever the scheduler does, an interval clock cannot
    // report more than the time that actually passed between the two reads. A
    // clock reporting a timestamp repeats the twenty milliseconds it just
    // measured, which no window around two adjacent lines can contain.
    assert!(
        immediately_after <= between,
        "two consecutive reads spanning {between:?} measured {immediately_after:?}; \
         the clock is reporting a timestamp, not an interval"
    );

    // And it keeps doing so: a hundred back-to-back readings partition the
    // window they are taken across, rather than each repeating the sleep.
    let outside = std::time::Instant::now();
    // Discarded, and that is what makes the bound hold: it closes the interval
    // that began before this window opened, so every reading summed below lies
    // inside it.
    clock.elapsed();
    let mut total = Duration::ZERO;
    for _ in 0..100 {
        total = total.saturating_add(clock.elapsed());
    }
    let span = outside.elapsed();
    assert!(
        total <= span,
        "a hundred immediate reads summed to {total:?} across {span:?}",
    );
}

#[cfg(feature = "std")]
#[test]
fn a_wall_clock_never_hands_a_step_more_time_than_actually_passed() {
    use core::num::NonZeroU32;

    use corvid_time::Clock;

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
    let mut clock = Clock::wall();
    // The ceiling must not be what bounds this test -- the elapsed time must
    // be -- so it is set where it can never bind, even if this thread loses the
    // processor for seconds between the sleep and the reads.
    let mut step = Step::new(rate).with_catchup(u32::MAX);
    let started = std::time::Instant::now();

    clock.elapsed();
    // Opened *after* the clock's first reading and closed *before* its last, so
    // this window lies strictly inside the one the clock measured. That is what
    // the lower bound needs: a window containing the clock's -- which is what
    // `started` gives -- grows whenever this thread is descheduled after the
    // final reading, and would fail the crate for the scheduler's behaviour.
    let inside = std::time::Instant::now();
    std::thread::sleep(Duration::from_millis(50));
    let mut ticks = 0u64;
    for _ in 0..19 {
        ticks += u64::from(step.advance(clock.elapsed()));
    }
    let measured = inside.elapsed();
    ticks += u64::from(step.advance(clock.elapsed()));
    let truth = started.elapsed();

    // The accumulator started empty and every interval the clock reported lies
    // between the first read and the last, both of which happen inside
    // `truth`. So the step was told about at most `truth` nanoseconds and can
    // have delivered at most the whole periods that fit in them -- no slack is
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
    // empty, and the ceiling cannot bind -- so fifty ticks were owed and fifty
    // were delivered. Asking for fewer would let a clock that under-reports by
    // a tenth through.
    assert!(
        ticks >= 50,
        "a fifty millisecond sleep yielded {ticks} ticks of a one millisecond period"
    );

    // The other direction, against `measured` rather than `truth`, because that
    // window is a subset of what the clock saw: every period that fits inside it
    // was reported to the step. One period of slack, because the two ends can
    // each straddle a boundary; a clock under-reporting by even a twentieth of
    // fifty periods misses by more than that.
    let owed = u64::try_from(measured.as_nanos() / u128::from(rate.nanos())).unwrap_or(0);
    assert!(
        ticks + 1 >= owed,
        "the step was told about {ticks} periods of the {owed} that really passed"
    );
}

#[test]
fn what_a_clock_reports_about_itself() {
    // Three accessors that nothing else here touches, and one of them decides
    // whether a caller trusts the reading it just took.
    let stepping = Clock::stepping(Duration::from_millis(7));
    assert_eq!(stepping.step(), Duration::from_millis(7));
    assert!(!stepping.is_wall());

    let still = Clock::still();
    assert_eq!(still.step(), Duration::ZERO);
    assert!(!still.is_wall());
}

#[cfg(feature = "std")]
#[test]
fn a_wall_clock_says_so_and_cannot_be_nudged() {
    use corvid_time::Clock;

    let mut clock = Clock::wall();
    assert!(clock.is_wall());
    assert_eq!(clock.step(), Duration::ZERO);

    // A wall clock has no queue to add to, and `advance` is documented as a
    // no-op rather than a panic so that one loop can drive either clock. Checked
    // as state rather than as a later reading: a `Clock` is `Clone` and `Eq`, so
    // "the nudge changed nothing" is exact, where any bound on what `elapsed`
    // says afterwards is a guess about the scheduler.
    let untouched = clock.clone();
    clock.advance(Duration::from_secs(100));
    assert_eq!(clock, untouched, "the nudge reached a wall clock");
}
