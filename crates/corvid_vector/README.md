# `corvid_vector`

Deterministic fixed-point 3-vectors for Corvid, built on
[`corvid_fixed`](https://docs.rs/corvid_fixed). Four concrete types, no
generics, every operation `const` and integer-only.

```rust
use corvid_fixed::{I24F8, I48F16};
use corvid_vector::{GlobalFinePoint, GlobalPoint};

// A camera on the earth's surface, and a point a millimetre from it.
let camera = GlobalFinePoint::splat(I48F16::from_f64(6_371_000.0));
let target = camera + GlobalFinePoint::new(
    I48F16::from_f64(0.001),
    I48F16::ZERO,
    I48F16::ZERO,
);

// The offset is exact: the range is spent on the absolute position, and the
// near-field difference still resolves to the last bit.
assert_eq!((target - camera).x().to_bits(), I48F16::from_f64(0.001).to_bits());

let p = GlobalPoint::new(I24F8::from_f64(3.0), I24F8::from_f64(4.0), I24F8::ZERO);
assert_eq!(p.length(), I24F8::from_f64(5.0));

// Normalizing is the one operation that can fail, and only at zero.
assert_eq!(GlobalPoint::ZERO.normalize(), None);
```

| Type | Component | Range | Resolution | Role |
|---|---|---|---|---|
| [`GlobalFinePoint`] | `I48F16` | +/-1.407e14 m | 15.26 um | Camera and VR pose position |
| [`GlobalPoint`] | `I24F8` | +/-8388 km | 3.9 mm | Object position; everyday offset |
| [`FinePoint`] | `I16F16` | +/-32.7 km | 15.26 um | Render and VR near-field |
| [`Direction`] | `Signed32` | unit | 4.7e-10 | Unit directions and rotation axes |

[`WideOffset`] is a fifth thing and not a point: the difference of two
[`GlobalPoint`]s, which is one bit wider than either. `GlobalPoint`'s own
subtraction saturates each axis independently, so a difference past the range
comes back as a different bearing rather than a shorter one -- and a caster whose
ray starts on the far side of the world needs the bearing. It answers ordinary
types ([`GlobalPoint`], `I24F8`, `I48F16`, [`Direction`]) and never a bit
pattern, so the widening stays a property of the arithmetic rather than of the
geometry. [`Volume`], the signed volume three offsets span, is the one quantity
here with no fixed-point type to be, and it is opaque for exactly that reason.

The names read as two independent axes: *Global* means wide range and *Fine*
means high resolution. Points double as offsets, the same choice Godot and Unity
make, because a separate offset type would double the API for no caught bug.

A squared length comes back in the widened unsigned intermediate rather than the
component type -- `u64` for the 32-bit components, `u128` for
[`GlobalFinePoint`] -- because [`GlobalPoint`]'s components reach 8388608 and
the sum of three squares passes `i64::MAX`. Expressing it back in `I24F8` would
saturate for any vector longer than 1672 m, which is worse than not offering the
operation. [`length`](GlobalPoint::length) and
[`distance`](GlobalPoint::distance) do return the component type, since a value
is its bit pattern over a fixed scale and the integer square root of the summed
squares is the answer's bit pattern.

Width conversions say in their names what they can do. Widening is exact,
narrowing the fractional part is total and rounds once, and narrowing the range
is checked and returns `None` only when the value does not fit -- never for
magnitude quietly discarded. [`GlobalFinePoint::to_fine`] is the one a renderer
runs thousands of times a frame, and because both types carry sixteen fractional
bits it is a pure range check with no rounding at all.

[`project`](GlobalPoint::project), [`align`](Direction::align) and
[`along`](Direction::along) are the mixed-scale products a raycast is built
from: how far along a direction an offset reaches, how much two directions
agree, and a direction walked a distance. All three fit an `i64`, which takes an
argument rather than a bound -- a `Direction` is a *unit* vector, so
Cauchy-Schwarz holds the sum of three Q39 products to `sqrt(3) * 2^62` and not
to `3 * 2^62`, and the difference between those two numbers is the difference
between fitting and not. `tests/project.rs` goes looking for the corner of the
world that reaches the bound rather than taking the algebra's word for it.

[`normalize`](GlobalPoint::normalize) returns `Option<Direction>`, `None` only
for the zero vector. Only the ratios of the components matter, so one
implementation serves all four widths without touching the component scale: one
[`I2F30::rsqrt`](corvid_fixed::I2F30::rsqrt), three multiplies and a few shifts,
with no division anywhere. Rescaling is a shift rather than a divide, so the
same direction at two magnitudes can differ in the last bit or two --
deterministic, not magnitude-independent to the bit.

The optional integrations are `mint`, `nalgebra`, `serde`, `bytemuck` and
`arbitrary`, all off by default.

## Scope

Three-component vectors at the three widths a world needs, plus the unit
direction they normalize into. No 2- or 4-component types and no generics: every
operation is `const`, `const fn` takes no trait bound, and each width is a
concrete type with an exhaustive suite behind it.

Positions, offsets and directions, and the arithmetic that stays inside them.
Rotating one is `corvid_rotation`, moving one between frames is
`corvid_transform`, and the `f32` vectors a shader reads are `corvid_glm`.
