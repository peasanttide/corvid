# `corvid_color`

Colour, as data, for a [Corvid](https://github.com/peasanttide/corvid) game.
Client ring, `no_std`, fixed-point, and behind no platform: a `LinearRgba` is
four `I16F16`s a game writes into its own uniform buffer, and this crate has no
opinion about which buffer that is.

```rust
use corvid_color::{LinearRgba, Rgba8};

// Authored in the form a palette is written in, converted where a shader
// wants it.
const EMBER: Rgba8 = Rgba8::opaque_hex(0xE5_78_29);
const LINEAR: LinearRgba = EMBER.to_linear();

// The crossing is exact in both directions, for all 2^3^2 colours.
assert_eq!(LINEAR.to_srgb8(), EMBER);

// And the one place a float appears is the boundary with a device.
let uniform: [f32; 4] = LINEAR.to_f32_array();
```

## Three representations, because they answer three questions

| Type | Holds | For |
|---|---|---|
| [`Rgba8`] | four `u8`, sRGB-encoded | authoring, storage, goldens, hashing |
| [`LinearRgba`] | four `I16F16`, linear | shaders, compositing, adding light |
| [`Oklab`] / [`Oklch`] | three `I2F30` and a coverage | generating a palette, mixing a ramp |

The distinction is not decoration. Light adds and sRGB codes do not, so
averaging two `Rgba8`s directly gives a colour darker than either; that is the
crossing `to_linear` exists for. And mixing in *linear* light is right for
compositing and wrong for a ramp somebody has to look at -- half way from red to
green in linear light is a muddy olive, and in Oklab it is still a colour. So
there are three, and each one's documentation says which question it answers.

## Everything here is fixed point

Three things that buys, each worth having for a colour:

- **It compares.** Every type here is `Eq` and `Ord`, where a floating-point
  colour could only ever be `PartialEq` -- and a `NaN` channel would compare
  equal to nothing at all, including itself.
- **It hashes.** `f32` and `f64` have no `Hash`, so a float in a hashed
  structure is a compile error rather than a desync between two targets. A
  fixed-point colour goes in a golden, in a UI layout digest and in a capture;
  a floating-point one could not.
- **It has no `NaN`.** Every operation saturates, so a colour arriving from a
  readback or a file has two failure modes rather than three, and the third -- a
  value that poisons every comparison it touches -- is not expressible.

`LinearRgba::to_f32_array` is the one place a float appears, and it sits on the
same boundary `corvid_render::matrix` does: everything above is fixed point and
everything below is what a GPU has.

## What `core` does not have, and what is done about it

A power of 2.4, a cube root, and an `atan2` -- none of which has a fixed-point
form either. This crate takes no `libm` dependency and uses no float for them:

- the sRGB transfer function is a **256-entry table** of `I16F16` bit patterns,
  which also makes [`decode`] a `const fn` -- so a palette's linear form can be a
  `const` rather than something computed at start-up;
- the cube root is Newton's method on `g^3 = x`, worked on the bit patterns in
  `i128` so nothing rounds through a narrower type on the way;
- the arctangent and its inverse are `corvid_fixed`'s integer trigonometry,
  which is why an [`Oklch`] hue is an `Angle32`. That is the better type
  regardless: a hue wraps, and an angle that says so cannot be interpolated the
  long way round by accident.

Sixteen fractional bits is the resolution question for a linear channel, and it
is settled at the dark end: sRGB code 1 is twenty steps and code 2 is forty, so
the darkest codes -- where the transfer function is steepest and a colour space's
precision is usually spent -- are still twenty apart. Nothing collides, and
`tests/round_trip.rs` walks all 256 to say so.

## Four bytes, or sixteen, that a buffer can take

Every type here is `#[repr(C)]`, and under the `bytemuck` feature every one is
`Pod` and `Zeroable`. That is what lets a palette become the bytes of a vertex
attribute or a uniform buffer in a workspace that forbids `unsafe_code`: an
`Rgba8` is the four bytes of an `Unorm8x4` attribute and a `LinearRgba` is the
four `i32` of a uniform, and `bytemuck::cast_slice` is the whole of the
crossing. The feature is off by default and pulls no `std`.

## Generating a palette

```rust
use corvid_color::{Oklch, Rgba8};
use corvid_fixed::{Angle32, I16F16, I2F30};

// Five hues, evenly spaced, all equally light and equally saturated -- which is
// the thing that is hard to do by hand and trivial in a polar perceptual space.
let wheel: [Rgba8; 5] = [0u32, 1, 2, 3, 4].map(|spoke| {
    let hue = Angle32::from_turns(f64::from(spoke) / 5.0);
    Oklch::new(I2F30::from_f64(0.7), I2F30::from_f64(0.15), hue, I16F16::ONE)
        .to_linear()
        .to_srgb8()
});

assert!(wheel.iter().all(|colour| *colour != Rgba8::BLACK));
assert_ne!(wheel[0], wheel[2]);
```
