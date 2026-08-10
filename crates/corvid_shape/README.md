# `corvid_shape`

Geometric primitives and the raycasts a cursor is resolved with, for a
[Corvid](https://github.com/peasanttide/corvid) game. Client ring, `no_std`,
and integer-only: the same fixed-point arithmetic the simulation uses, which is
what lets a picking test run with no GPU in the process.

The cast shapes are **world space**, in `GlobalPoint` -- `I24F8`, +/-8388 km at
3.9 mm an axis. A shape is an object, and an object is somewhere in the world
rather than somewhere relative to whoever happens to be looking at it.

[`Frustum`] is the exception, and deliberately so: it is **eye space**, in
`FinePoint`. A view volume is a property of whoever is looking, so its `contains`
and `intersects_sphere` take positions with the eye already subtracted -- which
is what a camera's own maths produces. Passing a world position to either is a
type error rather than a wrong answer, since the two are different types.

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
| [`Sphere`] | a ball -- the bounding volume for almost everything |
| [`Aabb`] | an axis-aligned box, and the slab test |
| [`Plane`] | a normal and an offset -- the ground, and half-spaces |
| [`Triangle`] | Moller-Trumbore, for picking a face out of a mesh |
| [`Frustum`] | a view volume in eye space, and the culling tests |

## Why there is no integer in here

Not one, and no bit pattern either. Every quantity in this crate is an `I24F8`,
an `I16F16`, a `Direction` or a `GlobalPoint`, and every operation on them is
named for the geometry rather than for the width it needs: a projection, an
alignment, a squared length, a squared closest approach, a signed volume.

That is a deliberate boundary rather than a tidiness. Casting geometry at the
scale of a world needs accumulators wider than the values going into it -- a
projection needs an `i64`, a triangle's scalar triple product needs more than
that -- and getting the bound wrong is not a rounding error. A cast that wrapped
would answer a hit *behind* the eye, which is a build cursor on the other side
of the world. So the widening belongs to the crates that own the scales:
`corvid_fixed` for the scalars and `corvid_vector` for the points, both of which
say in their own documentation how wide each operation goes and why.

What that leaves here is the geometry, which is the part worth reading.

The one thing this crate does have to know is that a difference of two points
can be wider than either of them. `WideOffset` is that difference, and every
method here that starts from a pair of far-apart points starts there --
`GlobalPoint`'s own subtraction saturates each axis independently, so a box
12 000 km across would come back with a centre 1 800 km off the middle.

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
`unsafe_code` -- so a bound that could not become bytes without it would be a
bound a game could not cull with. The feature is off by default and pulls no
`std`.

## What it does not do

No culling -- a cast at the inside of a planet's shell is a legitimate hit, and
`normal.align(ray.direction)` is the one line a caller writes if it disagrees.
No broad phase, no BVH, no spatial index: `Aabb` is the bound those are built
out of, and building them is a game's decision about its own world.
