# `corvid_float`

The floating-point maths a Corvid game needs at the boundary to its device.
`no_std`, and every function is `const`.

```rust
use corvid_float::{demote, sqrt, tan};

const DIAGONAL: f32 = sqrt(2.0);
const SLOPE: f32 = tan(0.5);
const NARROWED: f32 = demote(1.0 / 3.0);

assert!((DIAGONAL - 1.414_213_5).abs() < 1e-6);
```

`f32::sqrt` and its neighbours are compiler intrinsics, and an intrinsic cannot
be called in a `const`. So a projection matrix whose focal length is a `tan`
cannot be a `const` either, and in a workspace where a frustum, a colour and a
gain are all written down once and never changed, that is a constant recomputed
at every startup. The transcendentals here are software implementations from
[`const_soft_float`](https://crates.io/crates/const_soft_float) instead: slower
than the intrinsic at runtime, free at compile time.

Slower, not different. `sqrt`, `floor`, `ceil`, `trunc`, `round`, `abs` and
`copysign` come back bit-for-bit what the intrinsic returns. `sin`, `cos` and
`powi` land within one representable value of it, and `tan`, being two of those
divided by each other, within two. `hypot` is the one that genuinely differs, at
the far ends of the range, and its own documentation says where.

The `f32` surface is at the crate root and the `f64` one in [`wide`], and both
carry the same names. [`consts`] re-exports `core`'s `f32` constants, so a caller
reaching for a `PI` names one crate rather than two.

## Scope

The boundary, and not the workspace's arithmetic. Everything a Corvid simulation
hashes, sends or replays is fixed-point, because two machines have to agree on it
bit for bit and floating point does not give that across architectures. This
crate is for the other side of that line: the matrices, the texture coordinates
and the gains that reach a device, where nothing is compared against another
machine's answer and the rounding is free.

Scalars only. The vector and matrix types built on these are `corvid_glm`.
