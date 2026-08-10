# `corvid_fixed`

Deterministic fixed-point scalars for Corvid: fixed-point numbers, normalized
factors, signed normalized values, wrapping angles and clamping pitches, with
integer-only trigonometry. `no_std`, every operation `const` and total.

```rust
use corvid_fixed::{Angle16, Factor32, I24F8, Pitch16};

// A heading wraps, so turning past a full circle is not an error.
let mut yaw = Angle16::from_degrees(350.0);
yaw += Angle16::from_degrees(20.0);
assert_eq!(yaw.to_degrees().round(), 10.0);

// A pitch clamps, so looking too far up leaves you looking up.
let mut pitch = Pitch16::from_degrees(85.0);
pitch += Pitch16::from_degrees(20.0);
assert_eq!(pitch, Pitch16::MAX);

// Trigonometry with no libm in sight, correctly rounded to the last bit.
let (sin, cos) = yaw.sin_cos();
assert_eq!(Pitch16::asin(Pitch16::MAX.sin()), Pitch16::MAX);

// Overflow saturates by default; ask when you need to know.
let far = I24F8::from_f64(1_000_000.0);
assert_eq!(far * far, I24F8::MAX);
assert_eq!(far.checked_mul(far), None);

// Interpolation is exact at both ends.
let a = I24F8::from_f64(-10.0);
let b = I24F8::from_f64(10.0);
assert_eq!(a.lerp(b, Factor32::ZERO), a);
assert_eq!(a.lerp(b, Factor32::ONE), b);
```

Eighteen concrete types in five families, each a `#[repr(transparent)]` newtype
over an integer, so adding an [`Angle16`] to a [`Factor16`] is a type error:

| Family | Types | A value `v` denotes | Range |
|---|---|---|---|
| Fixed point | [`I0F8`], [`I8F8`], [`I24F8`], [`I16F16`], [`I48F16`], [`I2F30`] | `v / 2^FRAC_BITS` | per type, from `[-0.5, 0.5)` to `[-2^47, 2^47)` |
| Factor | [`Factor8`], [`Factor16`], [`Factor32`] | `v / MAX` | `0.0 ..= 1.0` |
| Signed | [`Signed8`], [`Signed16`], [`Signed32`] | `v / MAX` | `-1.0 ..= 1.0` |
| Angle | [`Angle8`], [`Angle16`], [`Angle32`] | `v / 2^BITS` turns | one full turn, wrapping |
| Pitch | [`Pitch8`], [`Pitch16`], [`Pitch32`] | `v / 2^BITS` turns | `-pi/2 ..= pi/2`, clamping |

Factors and signed values follow the GPU `UNORM`/`SNORM` convention, so their
bit patterns match `wgpu`'s. Angles are binary angle measurements, so wrapping
is free and no angle is invalid, and a pitch is the same scale with the ends
nailed down -- which makes [`to_angle`](Pitch16::to_angle) free and pairs a
pitch with a yaw in one unit.

Nothing panics. Division by zero saturates toward the numerator's sign, the
square root of a negative is zero, `NaN` converts to zero and infinities
saturate, so the operators can exist at all under the workspace's `panic =
"deny"` lint. Overflow saturates by default, and the `checked_`, `saturating_`,
`wrapping_` and `overflowing_` families name the behaviour when the default is
not what a caller wants.

Everything is `const`, including `sin`, `atan2`, `asin` and the roots, so a
table of geometry can be built at compile time. Floating point appears only in
the conversions -- `from_f64`, `to_f64`, `from_degrees`, `Display` -- which are
how a value gets in and out rather than how it is computed on. Multiplication,
division, the roots, the hypotenuse and interpolation each round once from a
full-width intermediate, and sine and cosine land on the bit pattern that rounding the true
value would give.

Trigonometry lives on the angle and pitch types, the only ones that know their
own units, and comes in two tiers. The exact tier is a seven-term Taylor series
over a folded octant in Q60 `i64`, with CORDIC for the arc functions. The
`_fast` tier trades accuracy for 32-bit-clean arithmetic: no widening multiply
and no operation WGSL lacks, so those algorithms transcribe into a shader and a
GPU can reproduce what the simulation computed. [`I2F30::rsqrt`] has the same
pair of tiers, and is the operation every normalize in the workspace actually
wants, where `x.sqrt().recip()` rounds twice.

Pi is not written down anywhere here. It is evaluated at compile time from
Machin's formula through a `const fn` arctangent series, as are the Taylor
coefficients and the CORDIC table.

The optional features are `serde`, `bytemuck`, `arbitrary` and `num-traits`, all
off by default. `nalgebra` needs no feature, since its blanket `Scalar` impl
already makes `Vector3<I24F8>` work; `RealField` is deliberately not
implemented, because it wants `exp`, `ln` and `powf`, which a fixed-point scalar
cannot answer honestly.

## Scope

Scalars, and nothing with more than one component. A 3-vector is
`corvid_vector`, a rotation `corvid_rotation`, a rigid transform
`corvid_transform`; each is built on these types rather than generic over them,
because `const fn` takes no trait bound.

Floating point only at the edges -- `corvid_float` is the other side of that
line. Rational and arbitrary-precision arithmetic, units and dimensional
analysis are all out. A nineteenth type arrives when something needs a width
these eighteen do not cover, rather than for symmetry.
