# `corvid_time`

Simulation time for [Corvid](https://github.com/peasanttide/corvid): a tick
number, the rate it is counted at, the fixed step that turns real time into
whole ticks, and the clock the simulation is never allowed to see.

Five types and one trait -- [`Tick`], [`Ticks`], [`TickSpan`], [`Step`],
[`Clock`] and [`Elapsed`]. Every one of them counts in integers -- nanoseconds
accumulated, periods taken out, a remainder left behind -- including the
sub-tick interpolation factor, which is the one quantity here that looks like it
wants to be a fraction.

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

## What a tick is

A [`Tick`] is an index, not a moment, and the type's own documentation is where
that argument lives. The wall-clock time a tick happened to run at is recorded
nowhere, because nothing deterministic may depend on it.

Its arithmetic saturates. `Tick(0).prev()` is `Tick(0)`, and
`Tick(60).since(Tick(100))` is zero rather than a wrapped `u64` a hair under
eighteen quintillion. A log indexes by `since`, so an answer that could go
backwards would have to be an `i64` that every caller then had to check;
ordering already answers "which came first", and `since` answers "how far".
Saturation at the top is thirty-nine billion years away at fifteen hertz and is
there only to keep the panic out.

[`Ticks`] is the other half of that pair: a count rather than a point in the
sequence, so `Tick(30)` is the thirty-first tick of a session and `Ticks(30)` is
thirty of them from wherever the counting started. It is what a caller passes to
ask a run to last a given number of ticks, and it has a frozen wire format of
its own -- `tests/wire.rs` pins it.

## The period is an integer, and that is the definition

[`TickSpan::from_hz`] truncates: the span it builds is `1_000_000_000 / hz`, and
the step accumulates against that same integer. The two cannot disagree, so
advancing by exactly one period a thousand times delivers exactly a thousand
ticks with nothing left over and nothing owed. A period that carried a remainder
the accumulator did not know about would leave a replay driven by its own rate's
period one tick short of the run it recorded, which is a desync produced by
arithmetic rather than by anything the game did.

What truncation costs is that the simulation runs fast against a wall clock, by
exactly `1_000_000_000 % hz` nanoseconds per second:

| Rate | `period()` | Fast by | One second of drift every |
|---|---|---|---|
| 10 Hz | 100 000 000 ns | -- | never |
| 15 Hz ([`CRADLE`](TickSpan::CRADLE)) | 66 666 666 ns | 10 ns/s | 3.2 years |
| 20 Hz | 50 000 000 ns | -- | never |
| 30 Hz | 33 333 333 ns | 10 ns/s | 3.2 years |
| 60 Hz | 16 666 666 ns | 40 ns/s | 289 days |
| 64 Hz | 15 625 000 ns | -- | never |
| 144 Hz | 6 944 444 ns | 64 ns/s | 181 days |

That is a trade of a quantity nobody measures for one everybody depends on. No
part of Corvid compares the tick counter to a calendar; every part of it
compares one run's tick counter to another's.

Fifteen hertz is low on purpose, and [`TickSpan::CRADLE`] is where that is
argued.

What a span stores is a [`NonZeroU32`](core::num::NonZeroU32) of nanoseconds,
and it is a hashable value with a wire format rather than a number in a config
struct because two peers at different spans are two different simulations.
Non-zero because a span of no time is a division by zero in [`Step`], and
thirty-two bits because that is what keeps [`Step::alpha`] inside a `u64`
without a clamp or 128-bit arithmetic -- [`TickSpan::MAX`] is four seconds and
change, against the one second that is the slowest span
[`from_hz`](TickSpan::from_hz) can name.

## A stall drops ticks; it does not bank them

[`Step::advance`] returns how many ticks are owed after some real time has
passed and refuses to return more than the catch-up ceiling; whatever it refuses
is *dropped* and counted in [`Step::dropped`], never carried forward, for the
reason set out at [`Step::advance`].

```rust
use corvid_time::{Duration, Step, TickSpan};

let span = TickSpan::CRADLE;
let mut step = Step::new(span).with_catchup(4);

// Ten seconds go missing -- a load, a breakpoint, a laptop lid. A hundred and
// fifty ticks are owed; four are delivered and a hundred and forty-six are gone.
assert_eq!(step.advance(Duration::from_secs(10)), 4);
assert_eq!(step.dropped(), 146);

// The point of dropping them: the next second is an ordinary second.
let mut ticks = 0;
for _ in 0..15 {
    ticks += step.advance(span.period());
}
assert_eq!(ticks, 15);
```

The remainder below one period does survive, so the ticks that *are* delivered
land on the schedule they would have without the stall, the interpolation factor
picks up where it left off, and only whole ticks past the ceiling are lost. The
default ceiling is eight, which is half a second at fifteen hertz;
[`Step::with_catchup`] takes another.

## Alpha, without a float in sight

[`Step::alpha`] is where the display sits between the last tick and the next:
zero immediately after a tick, climbing toward one as the next comes due. It is
a ratio of two integers rounded once onto a
[`Factor16`](corvid_fixed::Factor16), and [`Step::alpha`] is where that choice
is argued. There is no floating point in `src/` at all; a test reads the crate's
own source and fails if `f32`, `f64` or a decimal literal appears in it.

```rust
use core::num::NonZeroU32;
use corvid_fixed::Factor16;
use corvid_time::{Duration, Step, TickSpan};

// A rate cannot be zero, so building one from a plain number is a match rather
// than an unwrap. This is the idiom the workspace's lints leave you with.
const TEN_HZ: TickSpan = match NonZeroU32::new(10) {
    Some(hz) => TickSpan::from_hz(hz),
    None => TickSpan::CRADLE,
};

let mut step = Step::new(TEN_HZ);
assert_eq!(step.alpha(), Factor16::ZERO);

// Ten hertz is a period of 100 ms exactly, so a quarter of the way through it
// is a quarter of the factor's range: round(25 * 65535 / 100) is 16 384.
assert_eq!(step.advance(Duration::from_millis(25)), 0);
assert_eq!(step.alpha(), Factor16::from_bits(16384));

assert_eq!(step.advance(Duration::from_millis(75)), 1);
assert_eq!(step.alpha(), Factor16::ZERO);
```

## Clocks the simulation is not handed

Nothing here offers a simulation the time. A game that wants it has to go
looking, rather than find a clock among the things it was handed -- and a
simulation that read one would produce a different state on a slower machine,
so every save, replay and peer would disagree.

The absence narrows and does not forbid: a function that wants
`SystemTime::now()` can call it, and no signature anywhere stops that. What a
game does instead is keep its simulation crate free of anything that can reach a
clock, and check its ticks against each other.

[`Clock`] lives one level out, in the loop that drives the simulation, and its
whole purpose is that the loop can be handed a different one in a test:

| | Reads | For |
|---|---|---|
| [`Clock::stepping(period)`](Clock::stepping) | `period` every call, plus anything [`advance`](Clock::advance) has queued | Every headless test. One tick per iteration, forever |
| [`Clock::still`] + [`advance`](Clock::advance) | whatever was queued | Handing the loop an irregular or absurd frame time on purpose |
| [`Clock::wall`] (`std`) | the monotonic system clock | A game that is actually running |

[`elapsed`](Clock::elapsed) returns an interval rather than a timestamp. An
implementation that subtracts two absolute times is the one place a clock moving
backwards turns into a negative interval; returning the interval directly keeps
the answer unsigned the whole way. [`Clock::wall`] uses the monotonic clock, so
setting the system time does not move it, and saturates rather than panicking if
it ever appears to move backwards anyway.

Neither [`Clock`] nor [`Step`] is `Copy`, because both are consumed by being
read and a copy that gets read is time the original hands out twice; [`Clock`]
carries that argument in full. What swapping one for the other buys a test is
set out at [`Elapsed`].

## Features

All off by default. The crate is `no_std` and allocates nothing.

| Feature | Effect |
|---|---|
| `serde` | `Serialize`/`Deserialize` for [`Tick`], [`Ticks`] and [`TickSpan`], transparently as the number. A span of zero is refused by the deserializer rather than becoming a division by zero later |
| `std` | [`Clock::wall`]. The only feature that adds API |

[`Step`] and [`Clock`] have neither a wire format nor a digest, and the missing
`Hash` derive is deliberate rather than an omission: `corvid_hash::digest` takes
anything implementing `Hash`, so a derived one would have made
`digest(&Clock::wall())` compile. What these two hold is how far behind this
machine is, which is exactly the thing that must not enter a state two machines
are comparing.

The `serde` feature forwards to `corvid_fixed`; the manifest says why.

## Tests

```sh
cargo test -p corvid_time --all-features
```

| File | Covers |
|---|---|
| `tests/tick.rs` | Saturation at both ends, `since` in both directions, ordering, the period of thirteen rates against `1_000_000_000 / hz`, the residual identity the table above is built on, the gigahertz clamp, the JSON encoding, and ten thousand ticks digesting without a collision |
| `tests/step.rs` | A thousand exact periods at seven rates delivering a thousand ticks with nothing dropped and nothing accumulated; a period split into a hundred pieces; ten thousand ragged frame times accounting for every nanosecond; the stall dropping rather than banking; the ordinary second after it; the sub-tick remainder surviving that stall and completing the tick it was part of; alpha against hand-computed bit patterns, climbing within a period and landing on exactly zero at the boundary; a frame time of 2^64 nanoseconds saturating rather than wrapping; and the crate's own source containing no floating point |
| `tests/slow.rs` | The slow end of the range, which only `from_nanos` and the deserializer reach: that [`TickSpan::MAX`] is slower than any rate can name, and that alpha climbs rather than overflows across the whole range up to it |
| `tests/clock.rs` | `Clock` queueing and stepping, a thousand calls driving a thousand ticks, `Elapsed` as a trait object, a clock not being `Copy`, and the wall mode measuring a sleep and then measuring nothing at all on the next call |
| `tests/wire.rs` | The frozen bytes of [`Tick`], [`Ticks`] and [`TickSpan`], and the digests they come to under this workspace's hasher -- the one view of the three that can see a field narrowed |
| doctests | Every Rust block in this file and in the type documentation |
