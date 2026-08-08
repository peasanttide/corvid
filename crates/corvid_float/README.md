# `corvid_float`

The floating-point maths a [Corvid](https://github.com/peasanttide/corvid) game
needs at the boundary to its device. `no_std`, and **every function is
`const`**.

```rust
use corvid_float::{demote, sqrt, tan};

const DIAGONAL: f32 = sqrt(2.0);
const SLOPE: f32 = tan(0.5);
const NARROWED: f32 = demote(1.0 / 3.0);

assert!((DIAGONAL - 1.414_213_5).abs() < 1e-6);
```

## Why const, and what it costs

`f32::sqrt`, `f32::sin` and the rest are compiler intrinsics. An intrinsic
cannot be called in a const context, so a projection matrix whose focal length
is a `tan` cannot be a `const` — and in a workspace where a frustum, a colour
and a gain are all things a game writes down once and never changes, that is a
constant that has to be computed at every startup instead.

So the transcendentals here are software implementations, from
[`const_soft_float`](https://crates.io/crates/const_soft_float). They are slower
than the intrinsic at runtime and free at compile time, which is the trade this
crate is making: the values it is asked for are overwhelmingly the ones a
`const` wants.

Slower, not different. `sqrt`, `floor`, `ceil`, `trunc`, `round`, `abs` and
`copysign` come back bit-for-bit what the intrinsic returns — asserted on the
bits, over a sweep that visits every exponent an `f32` has, subnormals and
`MAX` included. `sin`, `cos` and `powi` land within one representable value of
the intrinsic and `tan`, being two of those divided by each other, within two.
`hypot` is the one that is genuinely different, at the far ends of the range,
and its own documentation says where and why.

`const_soft_float` supplies `sqrt`, `sin`, `cos`, `powi`, `floor`, `round`,
`trunc`, `copysign` and arithmetic. It has no `tan`, `ceil`, `hypot` or
`recip`; those are composed here, which is the other half of what this crate
is for.

## What it is not

It is not the workspace's arithmetic. Everything a Corvid simulation hashes,
sends or replays is fixed-point — `corvid_fixed` — because two machines have to
agree on it bit for bit and floating point does not give that across
architectures. This crate is for the other side of the boundary: the matrices,
the texture coordinates and the gains that reach a device, where nothing is
compared against another machine's answer and the rounding is free.

`corvid_glm` is the vector and matrix types built on top of it.
