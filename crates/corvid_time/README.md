# `corvid_time`

Simulation time for [Corvid](https://github.com/peasanttide/corvid): a tick
number, the rate it is counted at, the fixed step that turns real time into
whole ticks, and the clock the simulation is never allowed to see.

Four types and one trait. Every one of them counts in integers — nanoseconds
accumulated, periods taken out, a remainder left behind — including the
sub-tick interpolation factor, which is the one quantity here that looks like it
wants to be a fraction.

```rust
use corvid_time::{Clock, Duration, Fake, Step, Tick, TickSpan};

// Fifteen ticks a second, which is a period of 66 666 666 nanoseconds exactly.
let rate = TickSpan::CRADLE;
assert_eq!(rate.period(), Duration::from_nanos(66_666_666));

// A test drives the loop with a clock that passes one period per call, so a
// thousand iterations are a thousand ticks and the machine's speed is not part
// of the test.
let mut clock = Fake::stepping(rate.period());
let mut step = Step::new(rate);
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

A [`Tick`] is an index, not a moment. It says which step of the simulation a
state, an action, or a digest belongs to, and it means the same thing on every
peer replaying the same session — which is what lets a rollback name the tick it
is rolling back to and a golden trace name the tick it disagrees on. The
wall-clock time a tick happened to run at is recorded nowhere, because nothing
deterministic may depend on it.

Its arithmetic saturates. `Tick(0).prev()` is `Tick(0)`, and
`Tick(60).since(Tick(100))` is zero rather than a wrapped `u64` a hair under
eighteen quintillion. A log indexes by `since`, so an answer that could go
backwards would have to be an `i64` that every caller then had to check;
ordering already answers "which came first", and `since` answers "how far".
Saturation at the top is thirty-nine billion years away at fifteen hertz and is
there only to keep the panic out.

## The period is an integer, and that is the definition

[`TickSpan::nanos`] truncates: it is `1_000_000_000 / hz`, and the step
accumulates against that same integer. The two cannot disagree, so advancing by
exactly one period a thousand times delivers exactly a thousand ticks with
nothing left over and nothing owed. A period that carried a remainder the
accumulator did not know about would leave a replay driven by its own rate's
period one tick short of the run it recorded, which is a desync produced by
arithmetic rather than by anything the game did.

What truncation costs is that the simulation runs fast against a wall clock, by
exactly `1_000_000_000 % hz` nanoseconds per second:

| Rate | `period()` | Fast by | One second of drift every |
|---|---|---|---|
| 10 Hz | 100 000 000 ns | — | never |
| 15 Hz ([`CRADLE`](TickSpan::CRADLE)) | 66 666 666 ns | 10 ns/s | 3.2 years |
| 20 Hz | 50 000 000 ns | — | never |
| 30 Hz | 33 333 333 ns | 10 ns/s | 3.2 years |
| 60 Hz | 16 666 666 ns | 40 ns/s | 289 days |
| 64 Hz | 15 625 000 ns | — | never |
| 144 Hz | 6 944 444 ns | 64 ns/s | 181 days |

That is a trade of a quantity nobody measures for one everybody depends on. No
part of Corvid compares the tick counter to a calendar; every part of it
compares one run's tick counter to another's.

Fifteen hertz is low on purpose. A tick has sixty-six milliseconds to simulate
in, which is where a rollback of half a dozen ticks over fifty thousand entities
has to fit, and nothing the player's hands touch is waiting on it — the camera
and the cursor run at the display's refresh rate and never ask the simulation
for permission. Raising the rate spends the headroom rollback needs and buys
nothing that can be seen.

A rate is a [`NonZeroU32`](core::num::NonZeroU32) because zero has no period,
and it is a hashable value with a wire format rather than a number in a config
struct because two peers at different rates are two different simulations.

## A stall drops ticks; it does not bank them

[`Step::advance`] returns how many ticks are owed after some real time has
passed, and refuses to return more than the catch-up ceiling. Whatever it
refuses is *dropped* and counted in [`Step::dropped`], never carried forward.

```rust
use corvid_time::{Duration, Step, TickSpan};

let rate = TickSpan::CRADLE;
let mut step = Step::new(rate).with_catchup(4);

// Ten seconds go missing — a load, a breakpoint, a laptop lid. A hundred and
// fifty ticks are owed; four are delivered and a hundred and forty-six are gone.
assert_eq!(step.advance(Duration::from_secs(10)), 4);
assert_eq!(step.dropped(), 146);

// The point of dropping them: the next second is an ordinary second.
let mut ticks = 0;
for _ in 0..15 {
    ticks += step.advance(rate.period());
}
assert_eq!(ticks, 15);
```

The alternative is the spiral everyone writes once. A process that banks its
backlog hands the loop a thousand owed ticks, which take longer to simulate than
the stall took to happen, which leaves more owed at the end of the frame than at
the start. A ten-second pause then never ends. Dropping loses simulated time
that nobody was watching, and the frame after a stall is a frame like any other.

The remainder below one period does survive, so the ticks that *are* delivered
land on the schedule they would have without the stall, and the interpolation
factor picks up where it left off. Only whole ticks past the ceiling are lost.

The default ceiling is eight, which is half a second at fifteen hertz. A frame
that overruns its budget overruns it by a frame or two and eight is generous
room to make that back; a gap wider than half a second is not a frame problem
and is not improved by simulating half a second of a game nobody saw.

## Alpha, without a float in sight

[`Step::alpha`] is where the display sits between the last tick and the next:
zero immediately after a tick, climbing toward one as the next comes due. An
extractor interpolates the two states it was handed by this much, which is what
lets a fifteen-hertz simulation drive a hundred-and-forty-four-hertz display
without the picture stepping.

It is a ratio of two integers — nanoseconds accumulated over nanoseconds in a
period — rounded once onto a [`Factor16`](corvid_fixed::Factor16), the same
sixteen-bit factor the draw list, the vertex formats and the extractors already
carry. There is no floating point in the computation, and none anywhere else in
this crate either; a test reads the crate's own source and fails if `f32`, `f64`
or a decimal literal appears in it.

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

Nothing downstream of alpha is hashed, so determinism is not the argument here
— the argument is that a fraction computed in binary floating point would be
converted back to a sixteen-bit factor at both ends anyway, and would round on
the way through. The integer ratio is the shorter path and the exact one.

Rounding to sixteen bits means alpha reaches one within half of a factor's step
of the next tick rather than only at it — the last five hundred nanoseconds of a
sixty-six millisecond period, which no display can show.

## Clocks the simulation is not handed

`tick` is a free function with no `&self` and no clock among its arguments, so
the time is something a game has to go looking for rather than something the
contract hands it. A simulation that read a clock would produce a different state
on a slower machine and every save, replay and peer would disagree.

The absence narrows and does not forbid: a free function can call
`SystemTime::now()`, and no signature anywhere stops it. What a game does instead
is keep its simulation crate free of anything that can reach a clock and check
its ticks against each other; `corvid_behavior` is where that obligation is
stated and where the check lives.

[`Clock`] lives one level out, in the loop that drives the simulation, and its
whole purpose is that the loop can be handed a different one in a test:

| | Reads | For |
|---|---|---|
| [`Fake::stepping(period)`](Fake::stepping) | exactly `period`, every call | Every headless test. One tick per iteration, forever |
| [`Fake::new`] + [`advance`](Fake::advance) | whatever was queued | Handing the loop an irregular or absurd frame time on purpose |
| [`Wall`] (`std`) | the monotonic system clock | A game that is actually running |

[`elapsed`](Clock::elapsed) returns an interval rather than a timestamp. An
implementation that subtracts two absolute times is the one place a clock moving
backwards turns into a negative interval; returning the interval directly keeps
the answer unsigned the whole way. [`Wall`] uses the monotonic clock, so setting
the system time does not move it, and saturates rather than panicking if it ever
appears to move backwards anyway.

An interval is consumed by being read, which is why no clock here is `Copy` —
the same reason [`Step`] is not. A copy of a clock still holds the interval the
original just handed out, so passing one by value to a helper and then reading
the original delivers the same frame twice and the step counts it twice. `Clone`
stays on both: forking a second timeline off the same instant is a real thing to
want, and writing `clone` is the difference between doing it and doing it by
accident.

What this buys is that a headless run of ten thousand ticks finishes as fast as
the processor manages instead of in eleven minutes, and that a test about the
thousandth tick is a test about the thousandth tick rather than about the
machine it ran on.

## Features

All off by default. The crate is `no_std` and allocates nothing.

| Feature | Effect |
|---|---|
| `serde` | `Serialize`/`Deserialize` for [`Tick`] and [`TickSpan`], transparently as the number. A rate of zero is refused by the deserializer rather than becoming a division by zero later |
| `std` | [`Wall`]. The only feature that adds API |

[`Step`] and [`Fake`] have neither a wire format nor a digest, on purpose. They
are the runtime's business: what they hold is how far behind this machine is,
which is exactly the thing that must not enter a state two machines are
comparing.

`serde` forwards to `corvid_fixed`, so a downstream that names one crate gets
the whole stack rather than finding halfway through a state definition that its
factors are the one thing it cannot write down.

## Tests

```sh
cargo test -p corvid_time --all-features
```

| File | Covers |
|---|---|
| `tests/tick.rs` | Saturation at both ends, `since` in both directions, ordering, the period of thirteen rates against `1_000_000_000 / hz`, the residual identity the table above is built on, the gigahertz clamp, the wire format, and ten thousand ticks digesting without a collision |
| `tests/step.rs` | A thousand exact periods at seven rates delivering a thousand ticks with nothing dropped and nothing accumulated; a period split into a hundred pieces; ten thousand ragged frame times accounting for every nanosecond; the stall dropping rather than banking; the ordinary second after it; the sub-tick remainder surviving that stall and completing the tick it was part of; alpha against hand-computed bit patterns, climbing within a period and landing on exactly zero at the boundary; a frame time of 2^64 nanoseconds saturating rather than wrapping; and the crate's own source containing no floating point |
| `tests/clock.rs` | `Fake` queueing and stepping, a thousand calls driving a thousand ticks, `Clock` as a trait object, neither clock being `Copy`, and `Wall` measuring a sleep and then measuring nothing at all on the next call |
| doctests | Every Rust block in this file and in the type documentation |

That last row is not a formality: this README is the crate's front page, so
every `rust` block above is compiled and run by `cargo test`, and a claim that
stops being true stops the build.
