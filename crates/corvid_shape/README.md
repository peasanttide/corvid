# `corvid_shape`

Geometric primitives and the raycasts a cursor is resolved with, for a
[Corvid](https://github.com/peasanttide/corvid) game. Client ring, `no_std`,
and integer-only: the same fixed-point arithmetic the simulation uses, which is
what lets a picking test run with no GPU in the process.

Everything here is **world space**, in `GlobalPoint` — `I24F8`, ±8388 km at
3.9 mm an axis. A shape is an object, and an object is somewhere in the world
rather than somewhere relative to whoever happens to be looking at it. The
near-field `FinePoint` a renderer works in is what a camera's own maths
produces after the eye has been subtracted; it is not what a planet's cells are
stored in.

```rust
use corvid_shape::{Cast, Plane, Ray};
use corvid_fixed::I24F8;
use corvid_vector::{Direction, GlobalPoint, globalpoint};

// Where the cursor meets the ground.
let ground = Plane::through(GlobalPoint::ZERO, Direction::Z);
let cursor = Ray::new(globalpoint(3, 4, 10), -Direction::Z);
let hit = cursor.cast_against(&ground).expect("it points down");

assert_eq!(hit.point, globalpoint(3, 4, 0));
assert_eq!(hit.distance, I24F8::from_f64(10.0));
```

## What is here

| | |
|---|---|
| [`Ray`] | an origin and a unit direction, and `at` to walk it |
| [`Hit`] | a distance, a point, and a normal turned to face the ray |
| [`Cast`] | one method, so a game can cast at its own geometry too |
| [`Sphere`] | a ball — the bounding volume for almost everything |
| [`Aabb`] | an axis-aligned box, and the slab test |
| [`Plane`] | a normal and an offset — the ground, and half-spaces |
| [`Triangle`] | Möller–Trumbore, for picking a face out of a mesh |
| [`project`], [`align`] | the mixed-width dot products the rest is built from |

## Why every accumulator is an `i128`

A `GlobalPoint` component is an `I24F8` — a Q8 `i32` reaching ±8388 km at
3.9 mm — and a `Direction` component is a `Signed32`, a Q31 `i32`. Their product
is Q39 and reaches 2⁶², so **three of them summed do not fit an `i64`**: 3 × 2⁶² is a half
more than `i64::MAX`.

That bound is not theoretical. It is reached by a ray cast from near the edge of
the world along a diagonal, which is a cursor pointed at the horizon from the
far side of a planet. So
every dot and cross product here accumulates wide and narrows once, saturating
rather than wrapping — because a cast that saturates answers a hit at the far
edge of the world, and one that wrapped would answer a hit *behind the
eye*, which is a build cursor on the other side of the world.

## Three conventions worth knowing before reading a result

**A hit is always in front.** A shape entirely behind the ray's origin is a
miss rather than a negative distance. A quadratic solved without that check puts
the build cursor behind the player.

**A hit from inside is the exit.** A ray starting inside a sphere or a box
answers the far wall, from the inside, rather than nothing and rather than a
negative.

**A normal always faces the ray.** A hit on the inside of a sphere or the back
of a triangle answers the flipped normal, which is what a cursor decal and a rim
light both want. A caller that needs the geometric normal has the shape in hand
to ask it for one.

## Shapes a buffer can take

Every shape here is `#[repr(C)]` and has no padding, and under the `bytemuck`
feature every one is `Pod` and `Zeroable`. A broad phase that culls on the GPU
reads a list of `Aabb` or `Sphere` and nothing else, and this workspace forbids
`unsafe_code` — so a bound that could not become bytes without it would be a
bound a game could not cull with. The feature is off by default and pulls no
`std`.

## What it does not do

No culling — a cast at the inside of a planet's shell is a legitimate hit, and
`align(normal, ray.direction)` is the one line a caller writes if it disagrees.
No broad phase, no BVH, no spatial index: `Aabb` is the bound those are built
out of, and building them is a game's decision about its own world.
