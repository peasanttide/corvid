# `corvid_camera`

Ready-made pieces to compose into a [Corvid](https://github.com/peasanttide/corvid)
game's `Render::View`. Client ring, `no_std`, and behind no platform — a camera
here is a fixed-point pose and a frustum, and the 4×4 `f32` matrix a device
binds is built from the pair by [`matrix`] without a graphics library in
sight.

**A game is free to use none of this and write its own.** That is the point of
the crate being separate: a view is the game's type, and this is a box of parts
rather than a framework for cameras.

```rust
use corvid_camera::Orbit;
use corvid_fixed::{Angle32, I24F8, Pitch32};

let mut camera = Orbit::new(I24F8::from_f64(8.0));
camera.turn(Angle32::from_turns(0.1), Pitch32::from_turns(0.05));

// The eye is derived from the anchor and the facing, exactly, every frame — so
// it is always on the orbit, however fast the mouse is going.
let reach = camera.eye_position().distance(camera.anchor);
assert!((reach.to_f64() - 8.0).abs() < 0.01);
```

## What is here

| | |
|---|---|
| [`Camera`] | a pose and a frustum, and everything that follows from the pair |
| [`Orbit`] | anchor, facing, distance — the third-person camera, which produces one |
| [`FirstPerson`] | position, facing — the one with feet, which produces one too |
| [`Eye`] | the whole camera as the bytes a uniform block takes |
| [`matrix`] | the fixed-point-to-`f32` boundary |

A camera owns its frustum, so [`Camera`] is two fields — a pose and a frustum —
and the clip matrix, the uniform block and the picking ray all follow from the
pair by arithmetic.

It is a struct rather than a trait, and that is a decision worth knowing about.
Three separate contracts have to name this type: `Controller::look` answers one,
and `Render::draw` and `Auralizer::hear` are each handed one. A trait would have
meant either a type parameter threaded through all three and through the `App`'s
bounds, or a `Box<dyn Camera>` allocated once per displayed frame. Nothing was
lost — [`Orbit`] and [`FirstPerson`] have a `camera()` that answers one, and a
game that steers a camera its own way writes its own type and does the same.

The frustum itself is [`corvid_shape::Frustum`], which describes a
perspective frustum and an orthographic box with the same four numbers and no
tag saying which: the half-height at distance `d` is `base + slope * d`, and
the two cases are `base == 0` and `slope == 0`.

Cursor raycasting is [`Camera::ray`] plus `corvid_shape`, which a game reaches
through `corvid`: build the ray from the pose and the pointer, cast it at whatever the
game can be pointed at, and put the answer in the view. All of that is
client-ring, none of it is hashed, and it happens on the display's frame rather
than the simulation's — which is what "feel is local" means.

```rust
use corvid_camera::Orbit;
use corvid_fixed::{I16F16, Signed32};
use corvid_shape::Plane;
use corvid_vector::{Direction, GlobalPoint};

let camera = Orbit::default().camera();
let ground = Plane::through(GlobalPoint::ZERO, Direction::Z);

// Where the middle of the screen meets the ground. The aspect is a ratio, so
// it is an `I16F16` where the world the ray is cast into is `GlobalPoint`.
let ray = camera.ray((Signed32::ZERO, Signed32::ZERO), I16F16::ONE);
assert!(ray.cast_against(&ground).is_none()); // level, so it never arrives
```

## Two properties the steering holds, and why

**Adjacent yaws are adjacent.** `turn` builds its rotation with
`Versor::from_yaw_pitch_roll`, which multiplies the basis out in Q30 and has no
reject branch. Composing two half-angle quaternions instead would go through
`Versor::from_xyzw`, which rejects anything further from unit than 1.5e-5 —
and a sine and a cosine from `Angle16::sin_cos` miss `sin² + cos² = 1` by up to
4.3e-5, so 46% of the 65 536 representable yaws have no versor to build and no
sensible rotation to answer with. `tests/orbit.rs`'s
`every_yaw_is_adjacent_to_its_neighbour` freezes the property.

**The orbit is rigid and the anchor is what lags.** `ease_towards` moves the
anchor and nothing else, and `eye_position` is derived from it exactly every
frame. Easing
the eye while the facing was immediate would be the same camera described twice
and never at the same moment: at 81°/s — a two-pixel drag — the two are 138°
apart, which is an empty screen, and a faster spin pulls the eye inward far
enough to sit *inside* what it is watching. The player still sees a camera that
eases and settles, because it eases towards the thing that is actually moving,
and the framing does not depend on how fast the mouse is going or how fast the
display is. `easing_moves_the_anchor_only` and `the_eye_is_always_on_the_orbit`
freeze both halves.

## A stick and a mouse are not the same number

Nothing here multiplies by a frame's `dt`, and that is deliberate. A stick
reports a *deflection*, which is a rate, so a `look` multiplies it by `dt` on
the way in. A mouse reports the *motion that already happened*, which is a
quantity already proportional to how long the frame lasted — multiplying that by
`dt` again turns the camera by the square of the frame time, smoothly at a
steady rate and visibly as jitter the moment it wobbles.

`corvid_input` draws that line with two accessors, `analog` and `delta`. This
crate takes the angle it is given, so the decision stays where the binding is.
