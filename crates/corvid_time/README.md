# `corvid_time`

Simulation time for Corvid: a tick number, the rate it is counted at, the fixed
step that turns real time into whole ticks, and the clock the simulation is never
handed. `no_std`, and every quantity here is an integer.

```rust
use corvid_time::{Clock, Duration, Elapsed, Step, Tick, TickSpan};

// Fifteen ticks a second, which is a period of 66 666 666 nanoseconds exactly.
let span = TickSpan::CRADLE;
assert_eq!(span.period(), Duration::from_nanos(66_666_666));

// A test drives the loop with a clock that passes one period per call, so a
// thousand iterations are a thousand ticks and the machine's speed is not part
// of the test.
let mut clock = Clock::stepping(span.period());
let mut step = Step::new(span);
let mut tick = Tick::ZERO;

for _ in 0..1000 {
    for _ in 0..step.advance(clock.elapsed()) {
        tick = tick.next();
    }
}

assert_eq!(tick, Tick(1000));
assert_eq!(step.dropped(), 0);
```

A [`Tick`] is an index rather than a moment: the wall-clock time one ran at is
recorded nowhere, because nothing deterministic may depend on it. Its arithmetic
saturates, so `Tick(0).prev()` is `Tick(0)` and `since` never goes backwards.
[`Ticks`] is the other half of the pair, a count rather than a point.

A [`TickSpan`] stores a non-zero `u32` of nanoseconds, and
[`from_hz`](TickSpan::from_hz) truncates: the span is `1_000_000_000 / hz` and
the step accumulates against that same integer, so the two cannot disagree and a
thousand exact periods deliver a thousand ticks. What truncation costs is that
the simulation runs fast against a wall clock -- ten nanoseconds a second at
fifteen hertz, one second of drift every three years -- which is a quantity
nobody measures traded for one everybody depends on.

[`Step::advance`] returns the ticks owed after some real time has passed, up to a
catch-up ceiling. Whatever the ceiling refuses is *dropped* and counted in
[`Step::dropped`], never banked, so the second after a ten-second stall is an
ordinary second. The remainder below one period does survive, so the ticks that
are delivered land on the schedule they would have had anyway.
[`Step::alpha`] is where the display sits between two ticks, as a ratio of two
integers rounded once onto a [`Factor16`](corvid_fixed::Factor16). There is no
floating point in this crate at all, and a test reads its own source to keep it
that way.

Nothing here offers a simulation the time. [`Clock`] lives one level out, in the
loop that drives the simulation, and exists so that loop can be handed a
different one: [`Clock::stepping`] passes a fixed period per call for a headless
test, [`Clock::still`] passes only what [`advance`](Clock::advance) queued, and
[`Clock::wall`] reads the monotonic system clock. It returns an interval rather
than a timestamp, so a clock that appears to move backwards saturates instead of
going negative, and neither it nor [`Step`] is `Copy`, because both are consumed
by being read.

The optional `serde` feature writes [`Tick`], [`Ticks`] and [`TickSpan`]
transparently as their number; `std` adds [`Clock::wall`] and nothing else.
[`Step`] and [`Clock`] have no `Hash` and no wire format on purpose: what they
hold is how far behind this machine is, which is exactly what must not enter a
state two machines are comparing.

## Scope

Simulation time: the tick, the rate it is counted at, and the step that turns
elapsed nanoseconds into whole ticks. Civil time -- a date, a zone, a stamp a
person reads -- is not a tick and is not here.

No scheduling, no timers, and no callback after a delay.
