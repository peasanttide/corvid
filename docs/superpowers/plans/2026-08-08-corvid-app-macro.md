# `corvid::app!` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Collapse a Corvid game's `main` to a declaration of its five types, by
introducing a `Game` trait, one argument struct per hook, an optional seat, and
framework-driven bots.

**Architecture:** The five type parameters a game is (`State`, `Controller`,
`Bot`, `Render`, `Auralizer`) plus its tick span become one trait, `Game`, so
`App<S, C, R, A>` becomes `App<G>`. Two declarative macros write a `Game`
implementation: `game!` for a test, `app!` for a binary. Every hook a game
implements takes one struct rather than a list of arguments, so the seat that
bots need is a field rather than a signature change. `Seating` replaces the
implicit "this client always plays a seat" assumption with an explicit "always
watches one, sometimes plays one".

**Tech Stack:** Rust 2024, `wgpu`, `serde`/`serde_json`, `tracing`,
`thiserror`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-08-corvid-app-macro-design.md`

## Global Constraints

- Workspace lints deny `panic`, `unwrap`, `expect`, `print_stdout` and
  `print_stderr`. Every `#[allow]` and `#[expect]` carries a `reason = "…"`.
- Test files open with:
  ```rust
  #![allow(
      clippy::expect_used,
      clippy::unwrap_used,
      clippy::panic,
      reason = "a failed assertion in a test is a failed test, which is what a test is for"
  )]
  ```
- `Digestible` implementations are hand-written, never derived.
- Run `cargo fmt` after every hand-edit; the wrapping is not what you would type.
- Before any commit: `cargo fmt --check` and
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- Documentation is about what the code does now. **Do not write comments
  describing history** — no "this used to be", "was previously", "before the
  rewrite". Nothing here has been released.
- Nothing in this plan may move a digest. `examples/pong/tests/baseline.rs` is
  the evidence and must pass unchanged at every commit.

---

## File Structure

**New files**

| Path | Responsibility |
|---|---|
| `crates/corvid_time/src/ticks.rs` | `Ticks(u64)`, a count of ticks |
| `crates/corvid_app/src/game.rs` | The `Game` trait |
| `crates/corvid_app/src/seating.rs` | `Seating`, and which seat is watched or played |
| `crates/corvid_app/src/macros.rs` | `game!` and `app!` |
| `crates/corvid_app/src/record.rs` | Writing a session to one file |
| `crates/corvid_app/tests/seating.rs` | Spectator behaviour end to end |
| `crates/corvid_app/tests/bots.rs` | Bot seat filling end to end |
| `examples/pong/src/bot.rs` (extended) | `Opponent`, a `Controller<Table>` |

**Modified, and what changes**

| Path | Change |
|---|---|
| `crates/corvid_time/src/span.rs` | `TickSpan::from_millis` |
| `crates/corvid_behavior/src/extract.rs` | `Extracting`, `Extract::extract` |
| `crates/corvid_control/src/controller.rs` | `Acting`, `Updating`, three signatures |
| `crates/corvid_render/src/render.rs` | `Opened`, `Drawing`, two signatures |
| `crates/corvid_sound/src/auralizer.rs` | `Hearing`, `Auralizer::hear` |
| `crates/corvid_app/src/app.rs` | `App<G>`, `spectating`, `bots`, `state` |
| `crates/corvid_app/src/settings.rs` | `Settings<G>` with a fourth config |
| `crates/corvid_app/src/runtime.rs` | `Runtime<G, B>`, `Seating`, bot actions |
| `crates/corvid_app/src/windowed.rs` | `Windowed<G>`, `Pending<G>` |
| `crates/corvid_app/src/cli.rs` | `Arguments`, `Load`, `main::<G>` |
| `crates/corvid_app/src/saves.rs` | `saves` → `state` directory |
| `examples/pong/src/*` | The macro, `Opponent`, no `Hands::scripted` |

---

## Task 0: Establish the baseline

**Files:** none

- [ ] **Step 1: Confirm the tree is green before anything moves**

```bash
cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace --all-features
```

Expected: PASS. If it does not, stop and report — every later task compares
against this.

- [ ] **Step 2: Record pong's baseline digests**

```bash
cargo test -p pong --all-features --test baseline -- --nocapture
```

Expected: PASS. These digests must still pass at the end of every task below.

---

## Task 1: `Ticks` and `TickSpan::from_millis`

**Files:**
- Create: `crates/corvid_time/src/ticks.rs`
- Modify: `crates/corvid_time/src/lib.rs`, `crates/corvid_time/src/span.rs`
- Test: `crates/corvid_time/src/ticks.rs` (unit), `crates/corvid_time/src/span.rs` (doctest)

**Interfaces:**
- Consumes: nothing.
- Produces: `corvid_time::Ticks(pub u64)`;
  `corvid_time::TickSpan::from_millis(ms: u8) -> TickSpan`.

- [ ] **Step 1: Write the failing tests**

Create `crates/corvid_time/src/ticks.rs`:

```rust
//! How many ticks, as opposed to which one.

/// A count of ticks.
///
/// A count and a point in time are different things: `Tick(30)` is the
/// thirty-first tick of a session and `Ticks(30)` is thirty of them, from
/// wherever the counting started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(::serde::Serialize, ::serde::Deserialize),
    serde(transparent)
)]
pub struct Ticks(pub u64);

impl Ticks {
    /// No ticks at all.
    pub const NONE: Self = Self(0);

    /// The tick `self` ticks after `from`, saturating at the end of the range.
    #[must_use]
    #[inline]
    pub const fn after(self, from: crate::Tick) -> crate::Tick {
        crate::Tick(from.0.saturating_add(self.0))
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::panic,
        clippy::unwrap_used,
        reason = "a failed assertion in a test is a failed test, which is what a test is for"
    )]

    use super::Ticks;
    use crate::Tick;

    #[test]
    fn a_count_of_ticks_lands_that_far_past_where_it_started() {
        assert_eq!(Ticks(10).after(Tick(5)), Tick(15));
        assert_eq!(Ticks::NONE.after(Tick(5)), Tick(5));
    }

    #[test]
    fn a_count_that_would_run_off_the_end_stops_at_it() {
        assert_eq!(Ticks(u64::MAX).after(Tick(1)), Tick(u64::MAX));
    }
}
```

Add to `crates/corvid_time/src/span.rs`, inside `impl TickSpan`:

```rust
    /// The span of exactly this many whole milliseconds.
    ///
    /// The constructor a game writes. It is total because zero milliseconds is
    /// not a span: a `0` is taken as the shortest span there is, the same
    /// answer every other zero in this module gets, rather than as a division
    /// by zero in [`Step`](crate::Step).
    ///
    /// A game wanting a span no whole millisecond names — a 72 Hz headset's
    /// 13 888 888 ns — has [`from_nanos`](Self::from_nanos).
    ///
    /// ```
    /// use core::time::Duration;
    /// use corvid_time::TickSpan;
    ///
    /// const PONG: TickSpan = TickSpan::from_millis(33);
    /// assert_eq!(PONG.period(), Duration::from_millis(33));
    /// assert_eq!(PONG.hz(), 30);
    ///
    /// // Exact across the whole range.
    /// assert_eq!(TickSpan::from_millis(1).period(), Duration::from_millis(1));
    /// assert_eq!(TickSpan::from_millis(255).period(), Duration::from_millis(255));
    /// ```
    #[must_use]
    #[inline]
    pub const fn from_millis(millis: u8) -> Self {
        Self(nonzero(millis as u64 * 1_000_000))
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p corvid_time --all-features
```

Expected: FAIL — `ticks.rs` is not a module yet, `from_millis` is undefined.

- [ ] **Step 3: Wire the module up**

In `crates/corvid_time/src/lib.rs`, beside the existing module declarations and
re-exports:

```rust
mod ticks;

pub use ticks::Ticks;
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p corvid_time --all-features && cargo clippy -p corvid_time --all-targets --all-features -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add crates/corvid_time
git commit -m "time: a count of ticks, and a span named in milliseconds"
```

---

## Task 2: `Extracting`

**Files:**
- Modify: `crates/corvid_behavior/src/extract.rs`, `crates/corvid_behavior/src/lib.rs`
- Modify: `crates/corvid_app/src/runtime.rs:894,897`
- Modify: `crates/corvid_app/tests/common/mod.rs:439,483`
- Modify: `examples/pong/src/art.rs:215`, `examples/pong/src/play.rs:182`

**Interfaces:**
- Consumes: nothing.
- Produces: `corvid_behavior::Extracting<'a, S>` with public fields `state:
  &'a S`, `level: &'a S::Level`, `time: Time`; `Extract::extract(&mut self,
  extracting: Extracting<'_, S>)`.

- [ ] **Step 1: Rewrite the trait**

In `crates/corvid_behavior/src/extract.rs`, replace the trait and its `()`
implementation:

```rust
/// What an extractor is handed.
///
/// One struct rather than three arguments, so that a new thing to hand over is
/// a field here and not a signature change in every implementation.
///
/// [`Copy`], because two extractors are handed the same one per frame.
#[derive(Clone, Copy)]
pub struct Extracting<'a, S: State> {
    /// The state to read.
    pub state: &'a S,
    /// The level it is being played on.
    pub level: &'a S::Level,
    /// Where the session is.
    pub time: Time,
}

pub trait Extract<S: State> {
    /// Read out of a state whatever this half needs to draw or to sound.
    fn extract(&mut self, extracting: Extracting<'_, S>);
}

impl<S: State> Extract<S> for () {
    fn extract(&mut self, _extracting: Extracting<'_, S>) {}
}
```

Export it from `crates/corvid_behavior/src/lib.rs` beside `Extract`:

```rust
pub use extract::{Extract, Extracting};
```

- [ ] **Step 2: Run the build to see every caller break**

```bash
cargo build --workspace --all-features
```

Expected: FAIL, listing the four implementations and two call sites below.

- [ ] **Step 3: Fix the two call sites in the runtime**

`crates/corvid_app/src/runtime.rs`, around line 894, currently two `extract`
calls taking three arguments. Replace both with:

```rust
        let extracting = Extracting {
            state: &self.current,
            level: &self.play.session().opening.content,
            time,
        };
        if let Some(graphics) = self.graphics.as_mut() {
            graphics.extract(extracting);
        }
        self.ear.extract(extracting);
```

The second call needs the first not to have moved it, which is what the `Copy`
derive in Step 1 is for: the struct holds two shared references and a `Time`.

The borrow checker will object to `self.graphics.as_mut()` while `extracting`
borrows `self.current` and `self.play`. Build `extracting` from locals bound
before the `as_mut`:

```rust
        let state = Arc::clone(&self.current);
        let level = Arc::clone(&self.play.session().opening.content);
        let extracting = Extracting {
            state: &state,
            level: &level,
            time,
        };
```

Check whether `opening.content` is an `Arc` before writing this — read
`crates/corvid_replay/src/opening.rs`. If it is not, split the two calls into
separate statements each building their own `Extracting`.

- [ ] **Step 4: Fix the four implementations**

`examples/pong/src/play.rs:182`:

```rust
impl Extract<Table> for Ears {
    fn extract(&mut self, extracting: Extracting<'_, Table>) {
        self.contact = extracting.state.contact;
        self.at = extracting.state.now;
    }
}
```

`examples/pong/src/art.rs:215`, `crates/corvid_app/tests/common/mod.rs:439` and
`:483`: the same mechanical change — take `extracting: Extracting<'_, S>` and
read `extracting.state`, `extracting.level`, `extracting.time` where the three
arguments were read.

- [ ] **Step 5: Run the tests**

```bash
cargo test --workspace --all-features && cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: PASS, including `pong`'s `baseline`.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add -A
git commit -m "behavior: an extractor is handed one struct"
```

---

## Task 3: `Acting` and `Updating`, and the seat

**Files:**
- Modify: `crates/corvid_control/src/controller.rs`, `crates/corvid_control/src/lib.rs`
- Modify: `crates/corvid_app/src/runtime.rs:555,886`
- Modify: `crates/corvid_app/tests/common/mod.rs:354,806`
- Modify: `crates/corvid_control/tests/controller.rs:58`
- Modify: `crates/corvid_test/tests/common/mod.rs:298`
- Modify: `examples/pong/src/play.rs:112`, `examples/pong/src/rally.rs:95`
- Modify: `examples/pong/tests/baseline.rs:54`, `drawn.rs:82`, `linked.rs:116`

**Interfaces:**
- Consumes: nothing.
- Produces:
  ```rust
  pub type LevelRef<S> = <<S as State>::Level as Level>::Reference;

  #[derive(Clone, Copy)]
  pub struct Acting<'a, S: State> {
      pub state: &'a S,
      pub input: &'a Input,
      pub time: Time,
      pub seat: PlayerId,
  }

  pub struct Updating<'a, S: State> {
      pub state: &'a S,
      pub input: &'a Input,
      pub loading: Option<Loading<'a, LevelRef<S>>>,
      pub time: Time,
      pub dt: Duration,
      pub seat: PlayerId,
  }
  ```
  and `fn action(&self, acting: Acting<'_, S>) -> S::Action`,
  `fn update(&mut self, updating: Updating<'_, S>)`,
  `fn rumble(&self, acting: Acting<'_, S>) -> Option<RumbleId>`.

- [ ] **Step 1: Write the failing test**

In `crates/corvid_control/tests/controller.rs`, add:

```rust
#[test]
fn a_controller_is_told_which_seat_it_answers_for() {
    let hands = Hands::new(());
    let state = Walk::default();
    let input = Input::new(&[]);
    let time = Time::default();

    let first = hands.action(Acting {
        state: &state,
        input: &input,
        time,
        seat: PlayerId(0),
    });
    let second = hands.action(Acting {
        state: &state,
        input: &input,
        time,
        seat: PlayerId(1),
    });

    // `Hands` here ignores the seat, so both answers are the idle one — what
    // this pins is that the seat reaches the call at all.
    assert_eq!(first, second);
}
```

- [ ] **Step 2: Run it to verify it fails**

```bash
cargo test -p corvid_control --all-features
```

Expected: FAIL — `Acting` is not defined.

- [ ] **Step 3: Rewrite the trait**

In `crates/corvid_control/src/controller.rs`, make the alias public and add the
two structs above the trait:

```rust
/// How a state's level names itself, spelled once.
pub type LevelRef<S> = <<S as State>::Level as Level>::Reference;

/// What a controller is handed when it is asked for an action.
///
/// One struct rather than four arguments, so that a new thing to hand over is a
/// field here and not a signature change in every implementation.
#[derive(Clone, Copy)]
pub struct Acting<'a, S: State> {
    /// The state to read.
    pub state: &'a S,
    /// What the devices say, with every edge since the last tick folded in.
    pub input: &'a Input,
    /// Where the session is.
    pub time: Time,
    /// Which seat this answer is for.
    ///
    /// A controller playing one seat reads it or ignores it; a bot answering
    /// for several is called once per seat and this is how it tells them apart.
    /// It is here rather than on [`Time`] because a seat is not something a
    /// tick may read.
    pub seat: PlayerId,
}

/// What a controller is handed once per displayed frame.
pub struct Updating<'a, S: State> {
    /// The state to read.
    pub state: &'a S,
    /// What the devices say.
    pub input: &'a Input,
    /// How far along this machine's bytes are, while a level is being read.
    pub loading: Option<Loading<'a, LevelRef<S>>>,
    /// Where the session is.
    pub time: Time,
    /// Real time since the last displayed frame.
    pub dt: Duration,
    /// Which seat this controller is looking through.
    pub seat: PlayerId,
}
```

Change the three methods:

```rust
    fn update(&mut self, updating: Updating<'_, S>);

    fn action(&self, acting: Acting<'_, S>) -> S::Action;

    fn rumble(&self, _acting: Acting<'_, S>) -> Option<RumbleId> {
        None
    }
```

And the `()` implementation:

```rust
    fn update(&mut self, _updating: Updating<'_, S>) {}

    fn action(&self, acting: Acting<'_, S>) -> S::Action {
        let _ = acting;
        S::Action::default()
    }
```

Export from `crates/corvid_control/src/lib.rs`:

```rust
pub use controller::{Acting, Controller, LevelRef, Updating};
```

`Acting` needs `PlayerId`, which `corvid_behavior` already exports; add it to
the `use corvid_behavior::{…}` line at the top of `controller.rs`.

- [ ] **Step 4: Fix the two call sites in the runtime**

`crates/corvid_app/src/runtime.rs`, the `action` call around line 555:

```rust
        let action = self.controller.action(Acting {
            state: &self.current,
            input: self.acting(),
            time: self.now(),
            seat: self.seat,
        });
```

The `update` call around line 886:

```rust
        self.controller.update(Updating {
            state: &self.current,
            input: &self.input,
            loading: None,
            time,
            dt,
            seat: self.seat,
        });
```

- [ ] **Step 5: Fix the nine implementations**

Each takes the struct and reads its fields. `examples/pong/src/play.rs:112`:

```rust
    fn action(&self, acting: Acting<'_, Table>) -> Move {
        if let Some(scripted) = self.script(acting.time.tick) {
            return scripted;
        }
        match (
            acting.input.digital(action::UP).held,
            acting.input.digital(action::DOWN).held,
        ) {
            (true, false) => Move::Up,
            (false, true) => Move::Down,
            _ => Move::Still,
        }
    }

    fn update(&mut self, _updating: Updating<'_, Table>) {}
```

`examples/pong/src/rally.rs:95`, `examples/pong/tests/baseline.rs:54`,
`drawn.rs:82`, `linked.rs:116`, `crates/corvid_app/tests/common/mod.rs:354` and
`:806`, `crates/corvid_control/tests/controller.rs:58`, and
`crates/corvid_test/tests/common/mod.rs:298` take the same shape.

**`baseline.rs` keeps its own scripted paddle and its own arithmetic.** Change
only the signature; the `Move` it answers for a given tick must not change.

- [ ] **Step 6: Run the tests**

```bash
cargo test --workspace --all-features && cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: PASS. `pong`'s `baseline` digests must be unchanged — if they move,
an implementation's arithmetic was altered while its signature was.

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add -A
git commit -m "control: a controller is handed one struct, and told its seat"
```

---

## Task 4: `Opened` and `Drawing`

**Files:**
- Modify: `crates/corvid_render/src/render.rs`, `crates/corvid_render/src/lib.rs`
- Modify: `crates/corvid_app/src/app.rs:1029`, `crates/corvid_app/src/windowed.rs`
- Modify: `crates/corvid_app/src/screen.rs`, `crates/corvid_app/src/headless.rs`
- Modify: `crates/corvid_app/tests/common/mod.rs:487`
- Modify: `examples/pong/src/art.rs:237`

**Interfaces:**
- Consumes: `corvid_control::LevelRef` (Task 3).
- Produces:
  ```rust
  #[derive(Clone, Copy)]
  pub struct Opened<'a> {
      pub device: &'a wgpu::Device,
      pub queue: &'a wgpu::Queue,
      pub format: wgpu::TextureFormat,
  }

  pub struct Drawing<'a, S: State> {
      pub target: Target<'a>,
      pub camera: &'a Camera,
      pub loading: Option<Loading<'a, LevelRef<S>>>,
      pub time: Time,
      pub alpha: Factor16,
  }
  ```
  and `fn new(opened: Opened<'_>, config: Self::Config) -> Self`,
  `fn draw(&mut self, drawing: Drawing<'_, S>)`.

- [ ] **Step 1: Add the structs and change the trait**

In `crates/corvid_render/src/render.rs`:

```rust
/// The device a renderer builds its pipelines against.
#[derive(Clone, Copy)]
pub struct Opened<'a> {
    /// The device to create resources on.
    pub device: &'a wgpu::Device,
    /// The queue to submit on.
    pub queue: &'a wgpu::Queue,
    /// The surface's format, which is not the same on every machine.
    pub format: wgpu::TextureFormat,
}

/// What a renderer is handed for one frame.
pub struct Drawing<'a, S: State> {
    /// Where the frame goes.
    pub target: Target<'a>,
    /// Whatever the controller's `look` answered.
    pub camera: &'a Camera,
    /// How far along this machine's bytes are, while a level is being read.
    pub loading: Option<Loading<'a, LevelRef<S>>>,
    /// Where the session is.
    pub time: Time,
    /// The weight between the two extracted states: `ZERO` is the older.
    pub alpha: Factor16,
}
```

```rust
    fn new(opened: Opened<'_>, config: Self::Config) -> Self;

    fn draw(&mut self, drawing: Drawing<'_, S>);
```

and the `()` implementation:

```rust
    fn new(_opened: Opened<'_>, (): ()) -> Self {}

    fn draw(&mut self, _drawing: Drawing<'_, S>) {}
```

Export `Opened` and `Drawing` from `crates/corvid_render/src/lib.rs`.

- [ ] **Step 2: Run the build to see the callers break**

```bash
cargo build --workspace --all-features
```

Expected: FAIL at `app.rs`, `screen.rs`, `windowed.rs`, `common/mod.rs`,
`art.rs`.

- [ ] **Step 3: Fix `R::new` in `app.rs`**

`crates/corvid_app/src/app.rs`, in `run_offscreen` around line 1029:

```rust
        let graphics = R::new(
            corvid_render::Opened {
                device: renderer.device(),
                queue: renderer.queue(),
                format: renderer.format(),
            },
            settings.graphics.clone(),
        );
```

The same shape wherever `windowed.rs` builds the renderer.

- [ ] **Step 4: Fix the `draw` call**

Find the single `draw` call — it is in the `Backend` implementation that owns
the target, `crates/corvid_app/src/screen.rs`. Build a `Drawing` from the five
values it already has and pass it.

- [ ] **Step 5: Fix the two implementations**

`examples/pong/src/art.rs:237` and `crates/corvid_app/tests/common/mod.rs:487`
take `opened: Opened<'_>` and `drawing: Drawing<'_, S>` and read the fields
where the arguments were read.

- [ ] **Step 6: Run the tests**

```bash
cargo test --workspace --all-features && cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: PASS, including `pong`'s `drawn` golden — the pictures must be
identical.

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add -A
git commit -m "render: a renderer is handed one struct to open with and one to draw"
```

---

## Task 5: `Hearing`

**Files:**
- Modify: `crates/corvid_sound/src/auralizer.rs`, `crates/corvid_sound/src/lib.rs`
- Modify: `crates/corvid_app/src/runtime.rs:900`
- Modify: `crates/corvid_app/tests/common/mod.rs:449`
- Modify: `examples/pong/src/play.rs:189`

**Interfaces:**
- Consumes: nothing.
- Produces:
  ```rust
  pub struct Hearing<'a> {
      pub out: &'a mut AudioFrame,
      pub camera: &'a Camera,
      pub time: Time,
  }
  ```
  and `fn hear(&mut self, hearing: Hearing<'_>)`.

- [ ] **Step 1: Add the struct and change the trait**

In `crates/corvid_sound/src/auralizer.rs`:

```rust
/// What an ear is handed for one frame.
///
/// No type parameter: an ear reads the state through
/// [`Extract`](corvid_behavior::Extract) and writes cues here, so nothing in
/// this struct is the game's own type.
pub struct Hearing<'a> {
    /// The frame to write cues into.
    pub out: &'a mut AudioFrame,
    /// Where the listener is, which is where the eye is.
    pub camera: &'a Camera,
    /// Where the session is.
    pub time: Time,
}
```

```rust
    fn hear(&mut self, hearing: Hearing<'_>);
```

and the `()` implementation:

```rust
    fn hear(&mut self, _hearing: Hearing<'_>) {}
```

Export `Hearing` from `crates/corvid_sound/src/lib.rs`.

- [ ] **Step 2: Run the build to see the callers break**

```bash
cargo build --workspace --all-features
```

Expected: FAIL at `runtime.rs:900`, `common/mod.rs:449`, `play.rs:189`.

- [ ] **Step 3: Fix the call site**

`crates/corvid_app/src/runtime.rs` around line 900:

```rust
        self.ear.hear(Hearing {
            out: &mut self.audio,
            camera: &camera,
            time,
        });
```

- [ ] **Step 4: Fix the two implementations**

`examples/pong/src/play.rs:189`:

```rust
    fn hear(&mut self, hearing: Hearing<'_>) {
        hearing.out.listen(Listener::new(hearing.camera.pose));

        let Some(contact) = self.contact else {
            return;
        };
        let (sound, at) = match contact {
            Contact::Paddle { at, .. } => (KNOCK, at),
            Contact::Wall { at } => (THUD, at),
            Contact::Goal { .. } => (
                CHIME,
                FinePoint::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO),
            ),
        };
        if let Some(offset) = hearing.camera.pose.to_fine_global(at.to_global_fine()) {
            let id = hearing.out.next_id(self.at);
            hearing.out.cue(Cue::new(id, sound).at(offset));
        }
    }
```

`crates/corvid_app/tests/common/mod.rs:449` takes the same shape.

- [ ] **Step 5: Run the tests**

```bash
cargo test --workspace --all-features && cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: PASS. `pong`'s captured audio frames must be byte-identical.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add -A
git commit -m "sound: an ear is handed one struct"
```

---

## Task 6: The `Game` trait, and one type parameter everywhere

**Files:**
- Create: `crates/corvid_app/src/game.rs`
- Modify: `crates/corvid_app/src/lib.rs`, `app.rs`, `settings.rs`, `runtime.rs`, `windowed.rs`, `screen.rs`, `headless.rs`, `backend.rs`
- Modify: every `corvid_app` test that builds an `App`
- Modify: `examples/pong/tests/*.rs` that build an `App`

**Interfaces:**
- Consumes: everything from Tasks 2–5.
- Produces:
  ```rust
  pub trait Game {
      const PERIOD: TickSpan;
      type State: corvid_behavior::State + corvid_replay::Opens;
      type Controller: Controller<Self::State>;
      type Bot: Controller<Self::State>;
      type Render: Render<Self::State>;
      type Auralizer: Auralizer<Self::State>;
  }
  ```
  `App<G: Game>`, `Settings<G: Game>` with fields `controls`, `bot`, `graphics`,
  `audio`, `Outcome<G>`, `Runtime<G, B>`, `Windowed<G>`, `Pending<G>`.

- [ ] **Step 1: Write the trait**

Create `crates/corvid_app/src/game.rs`:

```rust
//! What a game is: five types and a tick span.

use corvid_behavior::State;
use corvid_control::Controller;
use corvid_render::Render;
use corvid_replay::Opens;
use corvid_sound::Auralizer;
use corvid_time::TickSpan;

/// The five types a game is, and how long its tick lasts.
///
/// A run names one of these instead of five parameters, which is what lets a
/// game's `main` be a declaration. [`app!`](crate::app) and
/// [`game!`](crate::game) write implementations of it.
///
/// # The bot
///
/// [`Bot`](Self::Bot) is a second [`Controller`], and a game with no bots names
/// `()` — which declares no actions, wants no devices and submits the idle
/// action forever. One instance answers for every seat a run gives it, told
/// which by [`Acting::seat`](corvid_control::Acting).
pub trait Game {
    /// How long one tick lasts. Every peer must agree.
    ///
    /// [`TickSpan::from_millis`] is what a game writes.
    const PERIOD: TickSpan;

    /// The deterministic half, and where a session starts.
    type State: State + Opens;

    /// Who is at the controls.
    type Controller: Controller<Self::State>;

    /// What plays the seats nobody is in.
    type Bot: Controller<Self::State>;

    /// What draws.
    type Render: Render<Self::State>;

    /// What sounds.
    type Auralizer: Auralizer<Self::State>;
}
```

Declare and export it in `crates/corvid_app/src/lib.rs`:

```rust
mod game;

pub use game::Game;
```

- [ ] **Step 2: Give `Settings` a fourth config and one parameter**

In `crates/corvid_app/src/settings.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Settings<G: Game> {
    /// What the controller is built from.
    pub controls: <G::Controller as Controller<G::State>>::Config,
    /// What the bot is built from.
    pub bot: <G::Bot as Controller<G::State>>::Config,
    /// What the renderer is built from, once there is a device.
    pub graphics: <G::Render as Render<G::State>>::Config,
    /// What the sound card is built from.
    pub audio: <G::Auralizer as Auralizer<G::State>>::Config,
}
```

Its `Default` gains the `bot` field and a fourth `Default` bound on the `where`
clause. `Settings::load` and `path` take `G` instead of the four parameters.

Spell these four associated types once as private aliases in `game.rs` and use
them everywhere, so the `<… as …>::Config` form appears in one file:

```rust
/// The controller's config, spelled once.
pub type Controls<G> =
    <<G as Game>::Controller as Controller<<G as Game>::State>>::Config;
/// The bot's config, spelled once.
pub type BotConfig<G> = <<G as Game>::Bot as Controller<<G as Game>::State>>::Config;
/// The renderer's config, spelled once.
pub type Graphics<G> = <<G as Game>::Render as Render<<G as Game>::State>>::Config;
/// The ear's config, spelled once.
pub type Audio<G> = <<G as Game>::Auralizer as Auralizer<<G as Game>::State>>::Config;
```

- [ ] **Step 3: Collapse the parameters**

Mechanical, in this order so the compiler leads you:

1. `App<S, C, R, A>` → `App<G: Game>`. Every `S` becomes `G::State`, `C` becomes
   `G::Controller`, `R` becomes `G::Render`, `A` becomes `G::Auralizer`. The
   `where C::Config: Default, …` clause becomes the four aliases.
2. `Outcome<S>` → `Outcome<G>` with `session: Session<G::State>` and
   `state: Arc<G::State>`.
3. `Runtime<S, C, R, A, B>` → `Runtime<G, B: Backend<G>>`, and
   `Backend<S, R>` → `Backend<G>`.
4. `Windowed<S, C, R, A>` and `Pending<S, C, R, A>` → `Windowed<G>`,
   `Pending<G>`.
5. `App::rate` reads `G::PERIOD` as its default rather than `TickSpan::CRADLE`.
   Keep the `rate` setter: a harness may run a game fast.

- [ ] **Step 4: Give every test a `Game`**

Each test that builds an `App` needs a marker type. Write them by hand for now —
Task 10 replaces these with `game!`. In `crates/corvid_app/tests/common/mod.rs`:

```rust
/// The game the tests in this crate play.
pub struct Counting;

impl corvid_app::Game for Counting {
    const PERIOD: corvid::TickSpan = corvid::TickSpan::CRADLE;
    type State = Tally;
    type Controller = Hands;
    type Bot = ();
    type Render = Painted;
    type Auralizer = Ears;
}
```

and `App::<Tally, Hands, Painted, Ears>::new()` becomes
`App::<Counting>::new()`. Where a test wants a different combination — a
headless variant with `type Render = ()` — give it its own marker beside this
one.

- [ ] **Step 5: Run the tests**

```bash
cargo test --workspace --all-features && cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: PASS, `baseline` unchanged.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add -A
git commit -m "app: a game is one type, so a run names one parameter"
```

---

## Task 7: `Seating`

**Files:**
- Create: `crates/corvid_app/src/seating.rs`, `crates/corvid_app/tests/seating.rs`
- Modify: `crates/corvid_app/src/app.rs`, `runtime.rs`, `lib.rs`

**Interfaces:**
- Consumes: `Game` (Task 6).
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub enum Seating {
      Playing(PlayerId),
      Watching(PlayerId),
  }
  impl Seating {
      pub const fn watched(self) -> PlayerId;
      pub const fn playing(self) -> Option<PlayerId>;
  }
  ```
  `App::spectating(self) -> Self`; `Error::NoSeats`.

- [ ] **Step 1: Write the failing test**

Create `crates/corvid_app/tests/seating.rs`:

```rust
//! What a client that watches a seat without playing it does.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Counting, opening};
use corvid_app::App;

/// The digests are the assertion: a spectator submits nothing, and a seat
/// nobody submits for holds the idle action — so the session a spectator
/// watches is the session an idle player would have produced.
#[test]
fn a_spectator_submits_nothing() {
    let watched = App::<Counting>::new()
        .opening(opening())
        .for_ticks(30)
        .spectating()
        .run()
        .expect("a spectating run");

    let idle = App::<Counting>::new()
        .opening(opening())
        .for_ticks(30)
        .run()
        .expect("a played run");

    // `Hands` in the common module answers the idle action for an empty input
    // snapshot, so the two sessions must agree tick for tick.
    assert_eq!(watched.session.marks, idle.session.marks);
}

#[test]
fn a_roster_with_no_seats_has_nothing_to_watch() {
    let empty = {
        let mut opening = opening();
        opening.roster = Vec::new();
        opening
    };
    let why = App::<Counting>::new()
        .opening(empty)
        .for_ticks(1)
        .spectating()
        .run()
        .expect_err("a roster with no seats");

    assert!(matches!(why, corvid_app::Error::NoSeats));
}
```

- [ ] **Step 2: Run it to verify it fails**

```bash
cargo test -p corvid_app --all-features --test seating
```

Expected: FAIL — `spectating` and `NoSeats` are undefined.

- [ ] **Step 3: Write `Seating`**

Create `crates/corvid_app/src/seating.rs`:

```rust
//! Which seat a client watches, and whether it plays it.

use corvid_behavior::PlayerId;

/// Where this client sits.
///
/// A client always watches a seat: the camera, the renderer and the ears belong
/// to somebody, and a run with nobody to look through has nothing to draw.
/// Whether it also submits an action for a seat is the other half, and it is
/// what `--spectator` decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Seating {
    /// Submits for this seat, and watches it.
    Playing(PlayerId),
    /// Submits for nobody, and watches this seat.
    Watching(PlayerId),
}

impl Seating {
    /// The seat this client's camera, renderer and ears belong to.
    ///
    /// Always one, which is why `update`, `look`, `draw` and `hear` never see
    /// an [`Option`].
    #[must_use]
    pub const fn watched(self) -> PlayerId {
        match self {
            Self::Playing(seat) | Self::Watching(seat) => seat,
        }
    }

    /// The seat this client writes an action for, if it writes one.
    #[must_use]
    pub const fn playing(self) -> Option<PlayerId> {
        match self {
            Self::Playing(seat) => Some(seat),
            Self::Watching(_) => None,
        }
    }
}

impl Default for Seating {
    fn default() -> Self {
        Self::Playing(PlayerId(0))
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::panic,
        clippy::unwrap_used,
        reason = "a failed assertion in a test is a failed test, which is what a test is for"
    )]

    use super::Seating;
    use corvid_behavior::PlayerId;

    #[test]
    fn a_player_watches_the_seat_it_plays() {
        let seated = Seating::Playing(PlayerId(2));
        assert_eq!(seated.watched(), PlayerId(2));
        assert_eq!(seated.playing(), Some(PlayerId(2)));
    }

    #[test]
    fn a_spectator_watches_a_seat_it_does_not_play() {
        let seated = Seating::Watching(PlayerId(1));
        assert_eq!(seated.watched(), PlayerId(1));
        assert_eq!(seated.playing(), None);
    }

    #[test]
    fn the_default_plays_the_first_seat() {
        assert_eq!(Seating::default(), Seating::Playing(PlayerId(0)));
    }
}
```

Declare and export it in `lib.rs`.

- [ ] **Step 4: Wire it into `App` and the runtime**

In `app.rs`, the `seat: PlayerId` field becomes `seating: Seating`. Keep
`App::seat`:

```rust
    /// Which seat this client submits an action for, and looks through.
    #[must_use]
    pub const fn seat(mut self, seat: PlayerId) -> Self {
        self.seating = Seating::Playing(seat);
        self
    }

    /// Watch a seat without playing it.
    ///
    /// The camera, the renderer and the ears are the watched seat's, and
    /// nothing is submitted for it: the column is filled by a peer or a bot, or
    /// holds the idle action. The seat watched is the roster's first, which is
    /// resolved when the run opens because that is when the roster is known.
    #[must_use]
    pub const fn spectating(mut self) -> Self {
        self.seating = Seating::Watching(PlayerId(0));
        self
    }
```

In `prepare`, after the session is opened:

```rust
        let seats = session.opening.roster.len();
        if seats == 0 {
            return Err(Error::NoSeats);
        }
        if usize::from(self.seating.watched().0) >= seats {
            return Err(Error::Seat {
                seat: self.seating.watched(),
                seats,
            });
        }
```

Add the variant to `Error`:

```rust
    /// The roster has no seats, so there is nobody to watch and no run.
    #[error("this session has no seats in its roster, so there is nobody to play and nobody to watch")]
    NoSeats,
```

In `runtime.rs`, `seat: PlayerId` becomes `seating: Seating`. Every read for a
camera or a hook uses `self.seating.watched()`. The write in `advance_alone`
becomes conditional:

```rust
        if let Some(seat) = self.seating.playing() {
            self.play
                .session_mut()
                .log
                .set(asked, seat, action)
                .map_err(Error::Log)?;
        }
```

And the `action` call in `advance` is skipped entirely when nothing is played:

```rust
        let action = self.seating.playing().map(|seat| {
            self.controller.action(Acting {
                state: &self.current,
                input: self.acting(),
                time: self.now(),
                seat,
            })
        });
```

`advance_alone` and `advance_linked` take `Option<G::Action>`. On the linked
path a `None` means the peer submits nothing and only receives.

- [ ] **Step 5: Run the tests**

```bash
cargo test -p corvid_app --all-features && cargo test --workspace --all-features
```

Expected: PASS, `baseline` unchanged.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add -A
git commit -m "app: a client always watches a seat and sometimes plays one"
```

---

## Task 8: Bots fill the empty seats

**Files:**
- Create: `crates/corvid_app/tests/bots.rs`
- Modify: `crates/corvid_app/src/app.rs`, `runtime.rs`

**Interfaces:**
- Consumes: `Seating` (Task 7), `Game::Bot` (Task 6).
- Produces: `App::bots(count: u16) -> Self`; `Runtime` holds one `G::Bot` and a
  `Vec<PlayerId>` of the seats it plays.

- [ ] **Step 1: Write the failing test**

Create `crates/corvid_app/tests/bots.rs`:

```rust
//! What a run with bots in it records.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Botted, opening};
use corvid_app::App;
use corvid_behavior::PlayerId;
use corvid_time::Tick;

#[test]
fn a_bot_takes_the_seat_this_client_is_not_playing() {
    let outcome = App::<Botted>::new()
        .opening(opening())
        .seat(PlayerId(0))
        .bots(1)
        .for_ticks(10)
        .run()
        .expect("a run with one bot");

    // The bot in `common` answers a non-default action for every tick, so the
    // seat it played has something in the log and the seat nobody played does
    // not.
    let log = &outcome.session.log;
    assert_ne!(log.get(Tick(0), PlayerId(1)), Some(Default::default()));
}

#[test]
fn a_spectator_lets_bots_take_every_seat() {
    let outcome = App::<Botted>::new()
        .opening(opening())
        .spectating()
        .bots(2)
        .for_ticks(10)
        .run()
        .expect("a run with two bots");

    let log = &outcome.session.log;
    assert_ne!(log.get(Tick(0), PlayerId(0)), Some(Default::default()));
    assert_ne!(log.get(Tick(0), PlayerId(1)), Some(Default::default()));
}

#[test]
fn more_bots_than_seats_fills_the_seats_there_are() {
    let outcome = App::<Botted>::new()
        .opening(opening())
        .spectating()
        .bots(99)
        .for_ticks(1)
        .run()
        .expect("a run asked for more bots than seats");

    assert_eq!(outcome.session.opening.roster.len(), 2);
}
```

Add a `Botted` marker to `crates/corvid_app/tests/common/mod.rs` whose `Bot` is
a controller answering a distinguishable action, and adjust
`log.get(…)`'s spelling to whatever `corvid_replay`'s log actually offers —
read `crates/corvid_replay/src/log.rs` for the accessor.

- [ ] **Step 2: Run it to verify it fails**

```bash
cargo test -p corvid_app --all-features --test bots
```

Expected: FAIL — `bots` is undefined.

- [ ] **Step 3: Add the builder call**

In `app.rs`:

```rust
    /// How many unclaimed seats the game's [`Bot`](crate::Game::Bot) plays.
    ///
    /// Bots take roster seats in order, skipping the seat this client is
    /// [`Playing`](crate::Seating::Playing). A spectator skips nothing: it
    /// watches a seat it does not play, and a bot may play the seat it watches.
    ///
    /// Asking for more bots than there are seats fills the seats there are.
    #[must_use]
    pub const fn bots(mut self, count: u16) -> Self {
        self.bots = count;
        self
    }
```

- [ ] **Step 4: Resolve the seats and drive them**

In `prepare`, after the roster is known:

```rust
        // Roster order, skipping the seat this client submits for. A spectator
        // skips nothing.
        let played = self.seating.playing();
        let bot_seats: Vec<PlayerId> = (0..seats)
            .filter_map(|seat| u16::try_from(seat).ok().map(PlayerId))
            .filter(|seat| Some(*seat) != played)
            .take(usize::from(self.bots))
            .collect();
```

Put `bot_seats` on the `Plan`. In `runtime.rs`, hold `bot: G::Bot` built from
`settings.bot.clone()` and `bot_seats: Vec<PlayerId>`, and in `advance_alone`
write the bots' actions alongside this client's:

```rust
        for seat in &self.bot_seats {
            let action = self.bot.action(Acting {
                state: &self.current,
                input: self.acting(),
                time: self.now(),
                seat: *seat,
            });
            self.play
                .session_mut()
                .log
                .set(asked, *seat, action)
                .map_err(Error::Log)?;
        }
```

after the `extend_to` and the client's own conditional write.

The linked path takes no bots: `--bots` with `--connect` is refused at the
command line in Task 9, and `App::bots` on an app with a
[`transport`](App::transport) is `Error::BotsAndPeers`. Add that variant:

```rust
    /// A run was asked for both bots and a transport.
    #[error(
        "this run has {bots} bots and a transport, and every peer running its own bots would \
         write the same seats' columns from controllers that are not hashed"
    )]
    BotsAndPeers {
        /// How many were asked for.
        bots: u16,
    },
```

Check it in `run`, before `prepare`.

- [ ] **Step 5: Run the tests**

```bash
cargo test -p corvid_app --all-features && cargo test --workspace --all-features
```

Expected: PASS, `baseline` unchanged.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add -A
git commit -m "app: bots take the seats nobody is in"
```

---

## Task 9: `Arguments`, `--record`, and `main::<G>`

**Files:**
- Create: `crates/corvid_app/src/record.rs`
- Modify: `crates/corvid_app/src/cli.rs`, `saves.rs`, `app.rs`, `lib.rs`
- Modify: `crates/corvid_app/tests/arguments.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: `Load`, the new `Arguments`, `App::state`, `App::record`,
  `Argument::Conflicting`, `Argument::NotALevel`, `corvid_app::main::<G>()`.

- [ ] **Step 1: Write the failing parser tests**

Replace `crates/corvid_app/tests/arguments.rs` with tests for the new surface.
The whole set, not a sample:

```rust
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]

use corvid_app::{Argument, Arguments, Load};
use corvid_behavior::{PlayerId, SaveSlot};
use corvid_time::Ticks;
use std::path::Path;

#[test]
fn nothing_at_all_is_every_default() {
    let parsed = Arguments::parse(Vec::<String>::new()).expect("no arguments");
    assert!(!parsed.headless);
    assert!(!parsed.spectator);
    assert_eq!(parsed.num_bots, 0);
    assert_eq!(parsed.ticks, None);
    assert_eq!(parsed.load, None);
    assert_eq!(parsed.record, None);
    assert_eq!(parsed.state, None);
    assert_eq!(parsed.seat, PlayerId(0));
    assert_eq!(parsed.listen, None);
    assert_eq!(parsed.connect, None);
}

#[test]
fn every_flag_is_read() {
    let parsed = Arguments::parse([
        "--headless",
        "--spectator",
        "--bots",
        "3",
        "--ticks",
        "90",
        "--record",
        "out/session",
        "--state",
        "here/",
        "--seat",
        "1",
        "--listen",
        "9000",
        "--connect",
        "host:9001",
    ])
    .expect("every flag");

    assert!(parsed.headless);
    assert!(parsed.spectator);
    assert_eq!(parsed.num_bots, 3);
    assert_eq!(parsed.ticks, Some(Ticks(90)));
    assert_eq!(parsed.record.as_deref(), Some(Path::new("out/session")));
    assert_eq!(parsed.state.as_deref(), Some(Path::new("here/")));
    assert_eq!(parsed.seat, PlayerId(1));
    assert_eq!(parsed.listen, Some(9000));
    assert_eq!(parsed.connect.as_deref(), Some("host:9001"));
}

#[test]
fn the_attached_spelling_is_the_same_argument() {
    let parsed = Arguments::parse(["--ticks=90", "--bots=2"]).expect("attached values");
    assert_eq!(parsed.ticks, Some(Ticks(90)));
    assert_eq!(parsed.num_bots, 2);
}

#[test]
fn each_way_of_opening_lands_in_the_one_field() {
    assert_eq!(
        Arguments::parse(["--load", "3"]).expect("a slot").load,
        Some(Load::Save(SaveSlot(3)))
    );
    assert_eq!(
        Arguments::parse(["--demo", "run/session"])
            .expect("a recording")
            .load,
        Some(Load::Demo("run/session".into()))
    );
    assert_eq!(
        Arguments::parse(["--level", "\"Court\""])
            .expect("a level")
            .load,
        Some(Load::Level("\"Court\"".to_owned()))
    );
}

#[test]
fn two_ways_of_opening_is_a_refusal_naming_both() {
    let why = Arguments::parse(["--load", "3", "--demo", "run/session"])
        .expect_err("two ways of opening");
    assert_eq!(
        why,
        Argument::Conflicting {
            flags: ["--load", "--demo"]
        }
    );
}

#[test]
fn bots_and_a_peer_is_a_refusal() {
    let why = Arguments::parse(["--bots", "1", "--connect", "host:9001"])
        .expect_err("bots and a peer");
    assert_eq!(
        why,
        Argument::Conflicting {
            flags: ["--bots", "--connect"]
        }
    );
}

#[test]
fn a_flag_that_takes_a_value_and_is_given_none_is_missing() {
    assert_eq!(
        Arguments::parse(["--ticks"]).expect_err("no value"),
        Argument::Missing { flag: "--ticks" }
    );
}

#[test]
fn a_value_on_a_flag_that_takes_none_is_refused() {
    assert_eq!(
        Arguments::parse(["--headless=false"]).expect_err("a value"),
        Argument::Unexpected { flag: "--headless" }
    );
}

#[test]
fn a_count_that_is_not_a_number_is_refused() {
    assert!(matches!(
        Arguments::parse(["--bots", "many"]).expect_err("not a number"),
        Argument::NotANumber { flag: "--bots", .. }
    ));
}

#[test]
fn asking_for_the_usage_is_reported_rather_than_printed() {
    assert_eq!(Arguments::parse(["-h"]).expect_err("help"), Argument::Help);
    assert_eq!(
        Arguments::parse(["--help"]).expect_err("help"),
        Argument::Help
    );
}
```

- [ ] **Step 2: Run them to verify they fail**

```bash
cargo test -p corvid_app --all-features --test arguments
```

Expected: FAIL — `Load` is undefined and the fields do not exist.

- [ ] **Step 3: Rewrite `Arguments`**

In `crates/corvid_app/src/cli.rs`, replace the struct, the `USAGE` text and the
`parse` body. Track which of the three opening flags was seen so the conflict
names both, and check `--bots` against `--connect` at the end of the loop:

```rust
/// What the run opens on.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Load {
    /// A level reference, as the JSON of `<S::Level as Level>::Reference`.
    Level(String),
    /// A save slot.
    Save(SaveSlot),
    /// A recorded session, which is what `--record` wrote.
    Demo(PathBuf),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Arguments {
    /// Open no window, no adapter and no audio device.
    pub headless: bool,
    /// Claim no seat: submit nothing, and watch the first one.
    pub spectator: bool,
    /// How many unclaimed seats the game's bot plays.
    pub num_bots: u16,
    /// Stop once this many ticks have run, counted from where the run opened.
    pub ticks: Option<Ticks>,
    /// What to open on, rather than the game's own opening.
    pub load: Option<Load>,
    /// Where to write the session, so that `--demo` can open it again.
    pub record: Option<PathBuf>,
    /// Where this game's files live, rather than the user data dir.
    pub state: Option<PathBuf>,
    /// Which seat this machine plays.
    pub seat: PlayerId,
    /// The UDP port to bind.
    pub listen: Option<u16>,
    /// Where the other machine is, as `HOST:PORT`.
    pub connect: Option<String>,
}
```

```rust
const USAGE: &str = "\
corvid: [--headless] [--spectator] [--bots N] [--ticks N]
        [--level JSON | --load N | --demo FILE] [--record FILE] [--state DIR]
        [--seat N] [--listen PORT] [--connect HOST:PORT]

  --headless        play with no window, no adapter and no audio device
  --spectator       claim no seat: submit nothing, and watch the first one
  --bots N          let the game's bot play N seats nobody is in
  --ticks N         stop once N ticks have run, counted from where the run
                    opened
  --level JSON      open on this level rather than the game's own
  --load N          open on save slot N
  --demo FILE       open on the session in FILE, which is what --record wrote,
                    and carry it on
  --record FILE     write the session to FILE as the run plays
  --state DIR       put this game's saves, settings and bindings under DIR
                    rather than the user data dir
  --seat N          which seat this machine plays
  --listen PORT     bind this UDP port
  --connect ADDR    the other machine, as HOST:PORT
  --help, -h        this";
```

Add the two error variants:

```rust
    /// Two flags that cannot both be acted on.
    #[error("{} and {} cannot both be given", flags[0], flags[1])]
    Conflicting {
        /// Which two.
        flags: [&'static str; 2],
    },
    /// A `--level` that is not JSON this game's level reference deserializes
    /// from.
    #[error("--level was given {value}, which is not a level this game has: {why}")]
    NotALevel {
        /// What was passed.
        value: String,
        /// Why it could not be read.
        why: String,
    },
```

`Argument` derives `PartialEq`, so `NotALevel` carries a `String` rather than a
`serde_json::Error`, which does not.

- [ ] **Step 4: Run the parser tests**

```bash
cargo test -p corvid_app --all-features --test arguments
```

Expected: PASS.

- [ ] **Step 5: Write the session recorder**

Create `crates/corvid_app/src/record.rs`:

```rust
//! Writing a session to one file, which is what `--demo` opens.

use std::path::Path;

use corvid_behavior::State;
use corvid_replay::Session;

use crate::Error;

/// Writes `session` to `path`.
///
/// The same bytes a capture's `session` file holds, so a `--record` and a
/// capture produce a file `--demo` and
/// [`replay`](crate::App::replay) read identically.
///
/// # Errors
///
/// [`Error::Encoded`] if the session will not encode, and [`Error::Wrote`] if
/// the file will not be written.
pub(crate) fn write<S: State>(path: &Path, session: &Session<S>) -> Result<(), Error> {
    let bytes = session.save().map_err(|why| Error::Encoded {
        what: "a session",
        why,
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|why| Error::Wrote {
            path: parent.to_path_buf(),
            why,
        })?;
    }
    std::fs::write(path, &bytes).map_err(|why| Error::Wrote {
        path: path.to_path_buf(),
        why,
    })
}
```

Add `App::record(path)`, storing an `Option<PathBuf>`, and call `record::write`
from `Runtime::finish` beside the existing capture close. A `record` implies
`Retention::Everything` — set it in `prepare` where the capture already does.

- [ ] **Step 6: Rewrite `main::<G>`**

Replace `cli.rs`'s `main` and `finish`:

```rust
pub fn main<G: Game>() -> crate::Result {
    watch();
    let Some(arguments) = command_line()? else {
        return Ok(());
    };
    let headless = arguments.headless;

    let mut app = App::<G>::new()
        .opening(<G::State as Opens>::opening())
        .rate(G::PERIOD)
        .input(corvid_input::Input::new(G::Controller::SETS))
        .bots(arguments.num_bots);

    app = if arguments.spectator {
        app.spectating()
    } else {
        app.seat(arguments.seat)
    };

    #[cfg(feature = "net")]
    if let (Some(port), Some(peer)) = (arguments.listen, arguments.connect.as_deref()) {
        app = app.transport(crate::net::udp(port, arguments.seat, peer)?);
    }

    #[cfg(feature = "window")]
    let app = if headless {
        app
    } else {
        app.window().bindings(G::Controller::bindings())
    };

    let outcome = app.arguments(arguments).run()?;
    finish::<G>(&outcome, headless);
    Ok(())
}
```

Move the UDP opening out of pong's `socket` into `crate::net::udp(port, seat,
peer) -> Result<Box<dyn Transport>, Error>`.

`finish` reports the settled digest:

```rust
/// How far back a reported digest is taken from.
///
/// Past a `Budget::DEFAULT`'s eight ticks ahead and two of delay, with room: a
/// state that far back was computed from actions every seat really submitted,
/// so it is the number two peers can be held to.
#[cfg(feature = "net")]
const SETTLED: u64 = 20;
/// A run with nobody else in it predicts nothing, so its last tick is settled.
#[cfg(not(feature = "net"))]
const SETTLED: u64 = 0;

fn finish<G: Game>(outcome: &Outcome<G>, headless: bool) {
    let last = outcome.session.last();
    let settled = Tick(last.0.saturating_sub(SETTLED));
    let mark = outcome
        .session
        .marks
        .get(settled)
        .map_or_else(|| "unknown".to_owned(), |mark| format!("{mark:#018x}"));

    tracing::info!(
        name: "corvid_app.finished",
        tick = %last,
        settled = settled.0,
        digest = %mark,
        requests = outcome.requests.len(),
        "the run ended",
    );
    #[cfg(feature = "net")]
    if outcome.traffic.heard != 0 || outcome.traffic.sent != 0 {
        tracing::info!(
            name: "corvid_app.netcode",
            heard = outcome.traffic.heard,
            sent = outcome.traffic.sent,
            rollbacks = outcome.traffic.rollbacks,
            resimulated = outcome.traffic.resimulated,
            deepest = outcome.traffic.deepest,
            stalls = outcome.traffic.stalls,
            "what the link cost",
        );
    }

    if headless {
        #[allow(
            clippy::print_stdout,
            reason = "this crate's `main` is a program rather than a library: an operator who passed `--headless` asked for this line on stdout, and a `main` of one line has nowhere to install a subscriber"
        )]
        {
            println!("{mark}");
        }
    }

    if outcome.exit != corvid_behavior::ExitCode::SUCCESS {
        std::process::exit(i32::from(outcome.exit.0));
    }
}
```

Match the `format!` to whatever `Digest`'s hex spelling actually is — read
`crates/corvid_hash/src/lib.rs` and use the same one `pong`'s current `report`
uses (`{:#018x}` over `mark.to_u64()`).

- [ ] **Step 7: Apply the new arguments**

In `app.rs`, `apply` reads the new fields: `headless`, `ticks` (`Ticks`),
`record`, `state`, `seat`/`spectator`, `num_bots`, and `load` matched three
ways. `Load::Level` deserializes with `serde_json::from_str` into
`LevelRef<G::State>` and replaces the opening's `level`. `App::saves` becomes
`App::state`; `Saves::resolve` takes the state root and joins `saves/`, and
`Settings::path` and `controls::resolve` take the same root.

- [ ] **Step 8: Add the round-trip test**

In `crates/corvid_app/tests/saves.rs`:

```rust
#[test]
fn a_recorded_session_is_one_a_demo_opens() {
    let pad = scratch();
    let file = pad.path().join("session");

    let first = App::<Counting>::new()
        .opening(opening())
        .for_ticks(20)
        .record(&file)
        .run()
        .expect("a recorded run");

    let second = App::<Counting>::new()
        .opening(opening())
        .replay(&file)
        .for_ticks(10)
        .run()
        .expect("a run carrying it on");

    assert_eq!(second.session.first(), first.session.first());
    assert_eq!(second.session.last().0, first.session.last().0 + 10);
    // The trace joins up: the tick the first run stopped at has the same mark
    // in both sessions.
    assert_eq!(
        second.session.marks.get(first.session.last()),
        first.session.marks.get(first.session.last())
    );
}
```

- [ ] **Step 9: Run everything**

```bash
cargo test --workspace --all-features && cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
cargo fmt
git add -A
git commit -m "app: the command line an operator types, and the main that acts on it"
```

---

## Task 10: `game!` and `app!`

**Files:**
- Create: `crates/corvid_app/src/macros.rs`
- Modify: `crates/corvid_app/src/lib.rs`, `src/lib.rs` (the `corvid` facade)
- Modify: `crates/corvid_app/tests/common/mod.rs` to use `game!`

**Interfaces:**
- Consumes: `Game` (Task 6), `App` (Tasks 6–9).
- Produces: `corvid::game!` and `corvid::app!`.

- [ ] **Step 1: Write the failing test**

In `crates/corvid_app/src/macros.rs`, as a doctest:

```rust
/// Declares a game: a struct, and the [`Game`](crate::Game) it implements.
///
/// ```
/// # use corvid_app::App;
/// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
/// # struct Nowhere;
/// # impl corvid_behavior::Level for Nowhere {
/// #     type Reference = String;
/// #     fn load(_: &String, _: &dyn corvid_files::Source)
/// #         -> Result<Self, corvid_files::Malformed> { Ok(Self) }
/// # }
/// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
/// # struct Tally;
/// # impl corvid_behavior::State for Tally {
/// #     const NAME: &'static str = "tally";
/// #     type Level = Nowhere; type Rules = (); type Action = ();
/// # }
/// # impl corvid_replay::Opens for Tally {
/// #     fn opening() -> corvid_replay::Opening<Self> { unimplemented!() }
/// # }
/// use corvid_time::TickSpan;
///
/// corvid_app::game! {
///     struct Counting;
///     const PERIOD: TickSpan = TickSpan::from_millis(66);
///     type State = Tally;
/// }
///
/// // Everything unnamed is `()`.
/// assert_eq!(<Counting as corvid_app::Game>::PERIOD, TickSpan::from_millis(66));
/// let _: App<Counting> = Counting::app();
/// ```
```

- [ ] **Step 2: Run it to verify it fails**

```bash
cargo test -p corvid_app --all-features --doc
```

Expected: FAIL — `game!` is undefined.

- [ ] **Step 3: Write the macros**

Because the five `type` lines may appear in any order and any may be absent, the
macro accumulates them with a `@collect` internal rule and emits once. Write it
as a `#[macro_export] macro_rules!` in `crates/corvid_app/src/macros.rs`:

```rust
/// Declares a game: a struct, the [`Game`](crate::Game) it implements, and a
/// sandbox constructor for a test.
///
/// `struct` and `const PERIOD` are required. Every `type` line is optional and
/// defaults to `()` — which draws nothing, hears nothing and submits the idle
/// action forever — and the lines may appear in any order.
#[macro_export]
macro_rules! game {
    (
        struct $name:ident;
        const PERIOD: $span:ty = $period:expr;
        $(type State = $state:ty;)?
        $(type Controller = $controller:ty;)?
        $(type Bot = $bot:ty;)?
        $(type Render = $render:ty;)?
        $(type Auralizer = $auralizer:ty;)?
    ) => {
        /// A game, declared by `corvid::game!`.
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        pub struct $name;

        impl $crate::Game for $name {
            const PERIOD: $span = $period;
            type State = $crate::game!(@or_unit $($state)?);
            type Controller = $crate::game!(@or_unit $($controller)?);
            type Bot = $crate::game!(@or_unit $($bot)?);
            type Render = $crate::game!(@or_unit $($render)?);
            type Auralizer = $crate::game!(@or_unit $($auralizer)?);
        }

        impl $name {
            /// A headless run with a scratch state directory and no settings
            /// file read.
            ///
            /// What a test wants: nothing about it depends on the machine it
            /// runs on, and one call stands for the builder lines every test
            /// file would otherwise repeat.
            #[must_use]
            pub fn app() -> $crate::App<Self> {
                $crate::App::<Self>::sandbox()
            }
        }
    };
    (@or_unit) => { () };
    (@or_unit $type:ty) => { $type };
}
```

This fixed-order form requires the lines in the order written. If a caller wants
them in any order, the macro needs a TT-muncher; write the muncher only if a
call site actually needs it, and keep the fixed order otherwise — every call
site in this workspace writes them in this order.

`app!` captures the struct's name so its `main` can name the type, and passes
everything through to `game!`:

```rust
/// Declares a game and the `main` that plays it.
///
/// The whole of a Corvid binary. Everything [`game!`](crate::game) accepts, plus
/// a `main` that reads the command line and decides the shape of the run.
#[macro_export]
macro_rules! app {
    (struct $name:ident; $($rest:tt)*) => {
        $crate::game! { struct $name; $($rest)* }

        fn main() -> $crate::Result {
            $crate::main::<$name>()
        }
    };
}
```

- [ ] **Step 4: Add `App::sandbox`**

In `app.rs`:

```rust
    /// A run that depends on nothing about the machine it is on.
    ///
    /// Headless, with the game's own opening, a state directory under the
    /// system's temporary directory keyed by the game's
    /// [`NAME`](corvid_behavior::State::NAME) and the process id, and
    /// [`Settings::default`] rather than whatever is in the player's file.
    ///
    /// This is what a test builds from. A run in front of a player is
    /// [`new`](Self::new).
    #[must_use]
    pub fn sandbox() -> Self {
        let root = std::env::temp_dir()
            .join(G::State::NAME)
            .join(std::process::id().to_string());
        Self::new()
            .opening(<G::State as Opens>::opening())
            .rate(G::PERIOD)
            .headless()
            .state(root)
            .settings(Settings::default())
    }
```

with the same `where` clause on the four configs the rest of that block carries.

- [ ] **Step 5: Re-export from the facade**

In `crates/corvid_app/src/lib.rs`:

```rust
mod macros;
```

`#[macro_export]` puts both at `corvid_app`'s root. In `src/lib.rs`, the
existing `pub use corvid_app::*;` carries them, so `corvid::app!` and
`corvid::game!` resolve. Add a test in the facade that proves it.

- [ ] **Step 6: Move the test markers onto `game!`**

Replace the hand-written `impl Game for Counting` in
`crates/corvid_app/tests/common/mod.rs` with a `game!` invocation, and the
builder lines in each test with `Counting::app()` where the test wants the
sandbox.

- [ ] **Step 7: Run everything**

```bash
cargo test --workspace --all-features && cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
cargo fmt
git add -A
git commit -m "app: two macros, so a game names its types once"
```

---

## Task 11: pong

**Files:**
- Modify: `examples/pong/src/main.rs`, `lib.rs`, `play.rs`, `bot.rs`, `rally.rs`
- Modify: `examples/pong/README.md`
- Modify: `examples/pong/tests/session.rs`, `socket.rs`, `bot.rs`, `together.rs`, `linked.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: `pong::Opponent`, a `Controller<Table>`; `pong::Pong`, a `Game`.

- [ ] **Step 1: Write `Opponent`**

In `examples/pong/src/bot.rs`, beside the existing `target` and `toward`:

```rust
/// An opponent that is actually trying.
///
/// A pure function of the state and the court, so a peer in a test, a peer in
/// `--together` and a seat filled by `--bots` all play the same paddle. What
/// reaches the wire is the [`Move`] it returns; nothing here is hashed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Opponent;

impl Controller<Table> for Opponent {
    type Config = ();

    const REAL: bool = false;
    const SETS: &'static [corvid::SetDescriptor] = &[];

    fn new((): ()) -> Self {
        Self
    }

    fn configure(&mut self, (): ()) {}

    fn update(&mut self, _updating: Updating<'_, Table>) {}

    fn look(&self) -> Camera {
        Camera::default()
    }

    /// Where the ball is going to be, and which part of the paddle to meet it
    /// with.
    fn action(&self, acting: Acting<'_, Table>) -> Move {
        let seat = usize::from(acting.seat.0);
        let court = court();
        let rules = rules();
        let target = target(seat, acting.state, &court, &rules);
        let at = acting
            .state
            .paddles
            .get(seat)
            .map_or(I16F16::ZERO, |paddle| paddle.at);
        toward(at, target, &court)
    }
}
```

Reading the court and the rules from `pong::court()` and `pong::rules()` rather
than from the session's opening keeps this a pure function of the state; if a
run ever plays a court that is not `court()`, take them from
`acting.state` instead — check whether `Table` carries them before writing this.

- [ ] **Step 2: Strip `Hands`**

In `examples/pong/src/play.rs`, delete `Hands::scripted`, `Hands::script` and
the `scripted` field. `type Config = ();`, `new` and `configure` take `()`, and
`action` reads only the input.

- [ ] **Step 3: Collapse `Racket` onto `Opponent`**

In `examples/pong/src/rally.rs`, `Policy::Chase` becomes `Opponent`. Keep
`Policy::Idle` as `()`. `Racket` either goes away or becomes a thin enum over the
two; whichever, `Match` must still play the same session — the `linked.rs` and
`together.rs` assertions are the check.

- [ ] **Step 4: Rewrite `main.rs`**

```rust
//! Plays pong: alone, against a bot, or against another machine.
//!
//! ```text
//! pong                                       one seat, a window, nobody opposite
//! pong --bots 1                              a window, and an opponent
//! pong --headless --spectator --bots 2       two bots, no window, one digest
//! pong --listen 9000 --connect HOST:9001     two machines, over a socket
//! ```

use corvid::TickSpan;
use pong::{Ears, Graphics, Hands, Opponent, Table};

corvid::app! {
    struct Pong;
    const PERIOD: TickSpan = TickSpan::from_millis(33);
    type State = Table;
    type Controller = Hands;
    type Bot = Opponent;
    type Render = Graphics;
    type Auralizer = Ears;
}
```

Delete `Ours`, `ours`, `usage`, `socket`, `socket_error`, `demo`, `together`,
`halted`, `report`, `netcode`, `SETTLED`, `OFFSCREEN`, `TICKS` and pong's own
`USAGE`.

- [ ] **Step 5: Remove `pong::RATE`**

In `examples/pong/src/lib.rs`, delete `RATE` and the `rate` helper, and export
`Opponent` from `bot`. Every test that named `RATE` names
`TickSpan::from_millis(33)` or the game's `Pong::PERIOD`.

- [ ] **Step 6: Move the tests off `Hands::scripted`**

`session.rs`, `socket.rs` and `bot.rs` build an `App` with
`.settings(Settings { controls: Some(seat), .. })`; they now use `.bots(n)` and
`Opponent`, or keep their own scripted controller where the point of the test is
a fixed script. **`baseline.rs` keeps its own `Scripted` and its own digests
unchanged.**

- [ ] **Step 7: Update the README**

`examples/pong/README.md` is included as the crate's module documentation, so
its flag table and its `--demo`/`--together` mentions must go. Replace them with
the four command lines from `main.rs`.

- [ ] **Step 8: Run everything, including release**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --workspace --all-features --release
```

Expected: PASS at every arm. `baseline` unchanged in both profiles — overflow
checks differ between them, so a debug-only pass is not a pass.

- [ ] **Step 9: Play it**

```bash
cargo run -p pong --all-features -- --headless --spectator --bots 2 --ticks 900
```

Expected: one digest on stdout, and `corvid_app.finished` on stderr.

```bash
cargo run -p pong --all-features -- --headless --spectator --bots 2 --ticks 900 --record /tmp/pong.session
cargo run -p pong --all-features -- --headless --spectator --bots 2 --ticks 100 --demo /tmp/pong.session
```

Expected: the second run carries the first on, and its digest differs because it
is 100 ticks further along.

- [ ] **Step 10: Commit**

```bash
cargo fmt
git add -A
git commit -m "pong: a main that is six lines of types"
```

---

## Self-Review Notes

**Spec coverage.** `Game` → Task 6. `Arguments`/`Load`/`--record`/`--state` →
Task 9. `Seating`/`NoSeats` → Task 7. Bots → Task 8. The five hook structs →
Tasks 2–5. `game!`/`app!`/`sandbox` → Task 10. pong, including the deletion of
`--demo`/`--together` and `Hands::scripted` → Task 11. `TickSpan::from_millis`
and `Ticks` → Task 1.

**Two things a task must discover rather than assume.** Task 8's
`log.get(tick, seat)` accessor and Task 9's `Digest` hex spelling are named from
the surrounding code but not read at planning time; each step says to read the
file. Task 11's `Opponent` needs to know whether `Table` carries its court, and
says so.

**The listening peer.** Task 7 changes `advance_linked` to take an
`Option<Action>`. If `corvid_lockstep::Peer` has no way to advance without
submitting, that is the one place this plan underestimates the work — stop and
report rather than inventing a submission.
