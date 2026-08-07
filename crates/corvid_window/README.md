# `corvid_window`

The `winit` half of Corvid: an event loop that owns `main`, a window whose state
is *published* rather than called back, and raw platform events turned into one
`corvid_input` snapshot per frame.

```rust
use corvid_input::Input;
use corvid_window::{Attached, Config, Flow, Host};

/// A game that stops as soon as it has drawn a hundred frames.
struct Counting {
    drawn: u32,
}

impl Host for Counting {
    // Nothing here can fail, and `Infallible` is how a host says so without
    // inventing an error nobody will ever construct.
    type Error = std::convert::Infallible;

    fn attach(&mut self, attached: &Attached) -> Result<Flow, Self::Error> {
        // A renderer is built here, because a window is what it needs and a
        // window does not exist until the platform says so.
        let _ = attached.surface.size();
        Ok(Flow::Go)
    }

    fn frame(&mut self, _input: &Input) -> Result<Flow, Self::Error> {
        self.drawn += 1;
        Ok(if self.drawn < 100 { Flow::Go } else { Flow::Stop })
    }
}

// `corvid_window::run(Config::new("bounce", SETS), Counting { drawn: 0 })`
// takes the thread from here and gives it back when the loop ends.
let host = Counting { drawn: 0 };
assert_eq!(host.drawn, 0);
```

## Why the loop owns `main`

On iOS, Android and the web the event loop *is* the program: the platform calls
into it, and a game that kept `main` would have nowhere to receive events. So
`run` takes the calling thread and hands control back one frame at a time. That
is one shape on five targets rather than two shapes on two.

The direction of the calls is what stops this being a window that runs a game.
Everything the loop hands over is data — a surface, a watch, an input snapshot —
and everything it gets back is a `Flow` and an error. The `Host` trait has no
method that could read a state, a tick or a digest, so a window cannot change
what a session computes. That is the property the whole design rests on, and
here it is a signature rather than a convention.

## State is published, not delivered

A resize is state: the latest size is the only one that matters. A callback
firing on every intermediate size during a drag would have a renderer
reconfigure its surface fifty times to arrive where one reconfigure would have
put it. So `SurfaceState` — size, scale factor, focus, occlusion — goes into a
`corvid_signal` cell, which keeps the latest value and drops the rest, and
whoever cares reads it once per frame.

`SurfaceState` publishes only when a field actually changed, because a platform
reports a window's size far more often than it changes and a publication is a
lock and an allocation.

## Input

`corvid_input`'s `platform` half is where the work is: a device-neutral
`Button` and `Axis`, a `Bindings` table between those and a game's declared
actions, and `Devices`, which accumulates events and works out the edges. This
crate's `src/translate.rs` is the only module in the workspace that names a
`winit` key code, so a binding file never learns which windowing library the
frame came from.

## Gamepads

`winit` reads a window's devices — a keyboard and a mouse — and a pad is
neither: it is an operating-system device, exposed through `evdev`, `XInput` and
`IOKit`. So the `gamepad` feature turns on `gilrs`, and `src/pad.rs` is the only
file in this workspace that names it.

What crosses out of that file is a `Button` and a deflection, which is exactly
what a key and a mouse cross as — so nothing downstream can tell a pad from a
keyboard. Two controls on one action union the same way, a binding file names a
pad button the same way, and a rebinding screen captures one the same way.

Two things it does *not* do. There is no rumble. And a `Button` carries no pad
number, so every pad folds into one set of controls: two pads are two hands on
one seat rather than two players, and splitting them needs a device-to-seat map
that a local-multiplayer game would design rather than inherit.

A stick gets a deadzone — a sixteenth, rescaled rather than clamped so the
control stays continuous across the edge — because a stick at rest does not
report zero and a game that believes it turns slowly for ever while nobody is
touching it.

Two decisions worth knowing about. Camera motion comes from `DeviceEvent::
MouseMotion` — the unaccelerated delta — rather than from where the cursor is,
because a cursor stops at the edge of the screen and a player sweeping into that
edge would find the camera stuck. And losing focus releases everything held: the
platform stops reporting releases the moment the player switches away, so
without it a key stays down until they come back and press it again.

### A stick and a mouse are not the same number

A stick reports a *deflection*, which is a rate: how fast to turn, so the frame's
`dt` multiplies it. A mouse reports the motion that *already happened*, which is
a quantity: the pixels a frame accumulated are already proportional to how long
that frame lasted, so multiplying by `dt` again turns the camera by the square of
the frame time. One accessor for both is a bug generator, so there are two —
`Input::analog` for the deflection and `Input::delta` for the displacement — and
a binding says which of them it answers on with `Reading`. This crate's tables
bind the mouse and the wheel as `Reading::Displacement`, because that is what
they report; an action bound that way reads zero from `analog`, which is a value
that stays still rather than a camera that shakes.

That is not a subtlety. `cargo run -p corvid_window --example jitter` injects two
thousand pixels of perfectly steady pointer motion through the X Test extension
and reports how far the camera turned, twice: once for a `look` that adds
`delta` as it stands, and once for the same per-frame number read as a deflection
and multiplied by `dt`. Both columns come from the same recorded frames of the
same run, so they differ in the arithmetic and in nothing else. The hand moves
the same distance in every row:

| Frames a second | Turn for 2000 px, `delta` | `analog × dt` |
|---|---|---|
| 176 000 | 9.362° | 0.005° |
| 216 | 9.366° | 4.542° |
| 82 | 9.373° | 12.115° |
| 41 | 9.373° | 24.481° |

The `delta` column is the same turn to within a twentieth of a degree across a
range of four thousand in frame rate, and what is left of even that is the
measurement's own: the sweep is cut at a frame boundary, so a fraction of one
frame's motion falls outside it. The other column moves by a factor of five
thousand. Re-running does not reproduce it exactly — a 41 Hz run measured
24.481° once and 24.615° the next time — because it is a function of how long
each frame actually took, which is the whole complaint.

Within the 41 Hz run, whose frames were deliberately uneven, each tenth of a
second of that steady sweep turns the camera by between 0.59° and 1.77° under
`analog × dt`: a factor of three between one tenth of a second and the next,
with nothing changing but which frames were long, and that is the shake. Under
`delta` the same tenths land inside a factor of 1.2 to 1.4 of each other, and
that residue is the report's own — a tenth of a second holds four frames at 41
Hz, so a window boundary carries up to a quarter of a frame's motion into its
neighbour. Both of those move a little from run to run and the ratio is the
thing to read.

`src/motion.rs` is what sits between the platform's events and the snapshot, and
with `delta` and `analog` separate there is one thing left for it to do. A binding's span —
`Bindings::placeholder`'s three hundred and twenty pixels — is how many device
units make a full sweep, and a frame that saw more than that would be clamped by
`Devices::snapshot`, which is motion thrown away rather than deferred. So it
holds what has been reported and not yet handed over, hands over at most a span
per frame, and carries the rest; past two frames' worth of debt it drops the
excess, because an axis pinned while a warp is paid off is a camera turning after
the hand stopped. Where one axis drives several actions the narrowest span is
what clamps, because only one number is handed over.

### The title and the icon are the game's, not this crate's

`Config::title` is what the title bar says and `Config::icon` is what the title
bar, the dock and the task switcher show. Neither is invented here and neither
is a builder argument a runtime passes twice: a game states its name once as
[`State::NAME`](corvid_behavior::State::NAME) and its picture once as
[`Render::icon`](corvid_render::Render::icon), and `corvid_app` is what
carries them across. An icon `winit` will not take is a warning and the
platform's own icon rather than a window that does not open, because a picture
is not a reason to refuse somebody a game.

### The default binding is a placeholder, and it says so

`Config::new` uses `Bindings::placeholder`, which binds **by identifier
number**: the digital actions of a declaration take a fixed list of keys in
order, the first analog action takes mouse motion, the second takes the wheel,
and everything past that is unbound. It has no idea what any of those actions
mean, and the key a player ends up pressing is an accident of where the action
was declared.

It exists so that a game with a window can be played before anything else in the
workspace exists. A real table — per device, rebindable, with glyphs, saved per
player — is not here. A game that wants one writes it out with
`Bindings::button` and `Bindings::axis` and passes it to `Config::bindings`.

## What is tested, and what is checked by hand

An event loop needs a display server, so `cargo test` cannot open a window. What
`cargo test` does check is every conversion that has no window in it: the
pointer normalisation and its clamp, the y flip between a window's downwards
coordinates and `Analog`'s upwards ones, a delta that is not a number, the key
translation, that every `Key` a game can bind is a key some physical key code
produces, and — the ones about relative motion — that one sweep of a given
length hands over the same total at 15, 60, 600, 1000 and 176 000 frames a
second, that a burst too fast for one frame's ceiling is deferred rather than
dropped, and that a warp is not deferred forever.

The numbers in the table above are not from those tests. They need a real
window, a real X server and real pointer events, so they come from
`examples/jitter`, which is in the repository for exactly that reason. The four
rows are four values of its argument — 0, 3, 8 and 16 milliseconds of sleep per
frame:

```sh
Xvfb :99 -screen 0 3000x2000x24 &
DISPLAY=:99 cargo run --release -p corvid_window --example jitter -- 16
```

Everything else is a manual check. `cargo run -p hello` on a machine with a
display is the check: a window opens, the cube falls and bounces, `Space` nudges
it, and moving the mouse orbits the camera. `examples/hello/README.md` says what
to look for.
