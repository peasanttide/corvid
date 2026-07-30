//! Deterministic fixed-point scalars: fixed-point numbers, normalized factors,
//! signed normalized values, wrapping angles, and clamping pitches.
//!
//! Corvid's simulation must produce identical results on every machine that
//! runs it, and floating point does not cooperate: compilers contract multiplies
//! into fused operations, libm's transcendental functions differ between
//! platforms, and `x87` and NEON round intermediate values differently. These
//! types are integers underneath, and every operation on them — including
//! trigonometry — is integer arithmetic. The same inputs give the same bits
//! everywhere.
//!
//! # The five families
//!
//! | Family | Types | A value `v` denotes | Range |
//! |---|---|---|---|
//! | [Fixed point](point) | [`I0F8`], [`I8F8`], [`I24F8`] | `v / 256` | `[-0.5, 0.5)`, `[-128, 128)`, `[-2^23, 2^23)` |
//! | [Factor](factor) | [`Factor8`], [`Factor16`], [`Factor32`] | `v / MAX` | `0.0 ..= 1.0` |
//! | [Signed](signed) | [`Signed8`], [`Signed16`], [`Signed32`] | `v / MAX` | `-1.0 ..= 1.0` |
//! | [Angle](angle) | [`Angle8`], [`Angle16`], [`Angle32`] | `v / 2^BITS` turns | one full turn, wrapping |
//! | [Pitch](pitch) | [`Pitch8`], [`Pitch16`], [`Pitch32`] | `v / 2^BITS` turns | `-pi/2 ..= pi/2`, clamping |
//!
//! They are separate types rather than aliases of one generic, so the compiler
//! rejects adding an [`Angle16`] to a [`Factor16`] and rustdoc shows real
//! signatures. All fifteen are `#[repr(transparent)]` newtypes over a primitive
//! integer, so they have the size, alignment, and layout of that integer.
//!
//! An [`Angle16`] is a heading: it wraps, so turning past a full circle is not an
//! error and never needs normalizing. A [`Pitch16`] is an elevation: it stops at
//! straight up and straight down, so looking too far up leaves you looking up
//! rather than flipping the world over. The two share a scale, so
//! [`Pitch16::to_angle`] is free and a camera's yaw and pitch are directly
//! comparable.
//!
//! Factors and signed values follow the GPU `UNORM`/`SNORM` convention: `MAX`
//! is exactly `1.0`. Their bit patterns match `wgpu`'s `Unorm8`, `Snorm16`, and
//! friends, so they cross into a vertex buffer unchanged. See [`signed`] for the
//! one wart that convention brings with it.
//!
//! # Nothing panics
//!
//! Every operation is total. There is no input — not division by zero, not the
//! square root of a negative, not `NaN`, not infinity — that panics or produces
//! a meaningless value.
//!
//! - `+ - * /` **saturate** at the bounds for the bounded families, and **wrap**
//!   for angles, where wrapping is the only thing a circle can do.
//! - `checked_*` returns `None` instead, for when leaving the range is a bug.
//! - `saturating_*`, and — for the families whose value space is a modular
//!   group — `wrapping_*` and `overflowing_*`, name the behavior explicitly.
//! - Division by zero saturates toward the numerator's sign; `0 / 0` is zero.
//! - `sqrt` of a negative is zero; [`checked_sqrt`](I24F8::checked_sqrt) reports
//!   it.
//! - Converting a `NaN` gives zero, and an infinity saturates.
//!
//! The families offer only the operations that mean something for them.
//! Multiplying two [`Factor16`]s cannot overflow, so the operation to reach for
//! is plain [`mul`](Factor16::mul) and there is nothing to choose between —
//! `checked_mul` and `saturating_mul` exist alongside it only because the
//! `num-traits` operator traits ask for them, and both just call `mul`.
//! Wrapping a [`Signed8`] past `1.0` is not a meaningful operation, so there is
//! no `wrapping_add`; and an [`Angle16`] has no bound to check against, so it
//! has no `checked_add`.
//!
//! # Beyond arithmetic
//!
//! The fixed-point family carries the operations you would reach for on an
//! `f64`, each exact or correctly rounded and none of them able to panic:
//! [`floor`](I24F8::floor), [`ceil`](I24F8::ceil), [`round`](I24F8::round),
//! [`trunc`](I24F8::trunc), [`fract`](I24F8::fract), [`abs`](I24F8::abs),
//! [`recip`](I24F8::recip), [`sqrt`](I24F8::sqrt),
//! [`hypot`](I24F8::hypot), and [`mul_add`](I24F8::mul_add) — which, like its
//! floating-point namesake, rounds once rather than twice. Every family has
//! [`min`](I24F8::min), [`max`](I24F8::max), [`clamp`](I24F8::clamp), and
//! [`lerp`](I24F8::lerp), the last of which follows the shortest arc on the
//! angle types.
//!
//! Trigonometry lives on the angle and pitch types, which are the only ones that
//! know their own units: [`sin`](Angle16::sin), [`cos`](Angle16::cos),
//! [`sin_cos`](Angle16::sin_cos), [`tan`](Angle16::tan),
//! [`atan2`](Angle16::atan2), [`acos`](Angle16::acos), and
//! [`asin`](Pitch16::asin) — whose result range is exactly a pitch's, which is
//! why it lives there. Each has a `_fast` counterpart that trades a little over
//! a thousandth of accuracy — `1.2e-3` for sine, `4.4e-3` radians for
//! arctangent — for a third of the time.
//!
//! # Speed
//!
//! Integer trigonometry is not a compromise here. Measured on aarch64 with
//! `cargo run --release --example bench`, against the platform's own `libm`:
//!
//! | Operation | Versus `f64` |
//! |---|---|
//! | [`sin`](Angle16::sin), [`cos`](Angle16::cos) | **0.71x** |
//! | [`sin_cos`](Angle16::sin_cos) | **0.98x** |
//! | [`sin_fast`](Angle16::sin_fast) | **0.20x** |
//! | [`tan`](Angle16::tan) | 1.75x |
//! | [`atan2`](Angle16::atan2) | 1.74x for [`Angle16`], 3.1x for [`Angle32`] |
//! | [`atan2_fast`](Angle16::atan2_fast) | **0.69x** |
//! | [`asin`](Pitch16::asin), [`acos`](Angle16::acos) | 6.8x |
//! | multiplication | 1.5x |
//! | [`sqrt`](I24F8::sqrt) | 12x |
//!
//! Sine and cosine beat the platform because a Taylor series over a folded octant
//! in `i64` is simply less work than a correctly-rounded `libm` reduction. Square
//! root loses because the hardware has an instruction for it and integer square
//! root is a loop. The arc functions carry a CORDIC loop, so they are the slowest
//! thing here; [`atan2_fast`](Angle16::atan2_fast) exists for when that matters.
//!
//! # Everything is `const`
//!
//! Every inherent operation is a `const fn`, trigonometry included, so tables of
//! angles and precomputed geometry can be built at compile time:
//!
//! ```
//! use corvid_fixed::{Angle16, Signed16};
//!
//! const SPOKES: usize = 8;
//! const DIRECTIONS: [(Signed16, Signed16); SPOKES] = {
//!     let mut spokes = [(Signed16::ZERO, Signed16::ZERO); SPOKES];
//!     let mut i = 0;
//!     while i < SPOKES {
//!         let phase = (i * (u16::MAX as usize + 1) / SPOKES) as u16;
//!         spokes[i] = Angle16::from_bits(phase).sin_cos();
//!         i += 1;
//!     }
//!     spokes
//! };
//!
//! assert_eq!(DIRECTIONS[0], (Signed16::ZERO, Signed16::MAX));
//! assert_eq!(DIRECTIONS[2], (Signed16::MAX, Signed16::ZERO));
//! ```
//!
//! Operator traits cannot be `const`, so `a.saturating_add(b)` works in a
//! `const` context where `a + b` does not.
//!
//! # Precision and round-trips
//!
//! `to_bits` and `from_bits` are exact inverses for every bit pattern, which is
//! what makes `bytemuck` and `serde` faithful.
//!
//! Going out through a float and back is lossless through `f64` for all fifteen
//! types: the widest carries 32 significant bits against `f64`'s 53. Through
//! `f32`, with 24 bits of mantissa, it is lossless only for the 8-bit and 16-bit
//! types. [`I24F8`], [`Factor32`], [`Signed32`], [`Angle32`], and [`Pitch32`]
//! need `f64`.
//!
//! The reverse direction — float to fixed to float — quantizes, so it is lossy
//! by construction. `0.25` is exact in [`I8F8`] and not in [`Factor16`], because
//! a factor's scale is `65535` rather than a power of two.
//!
//! Multiplication, division, square root, and interpolation each round once,
//! from a full-width intermediate, halfway away from zero. The result is the
//! representable value nearest the true one.
//!
//! # Features
//!
//! All are off by default. The crate is `no_std`, and stays that way with every
//! feature except `arbitrary`, whose derive macro emits `std` paths.
//!
//! | Feature | Effect |
//! |---|---|
//! | `serde` | `Serialize`/`Deserialize`, transparently as the raw integer |
//! | `bytemuck` | `Pod` and `Zeroable`, for casting to and from bytes |
//! | `arbitrary` | `Arbitrary`, for fuzzing and property tests (links `std`) |
//! | `num-traits` | `Zero`, `One`, `Bounded`, `ToPrimitive`, `FromPrimitive`, and the checked, saturating, and wrapping operator traits |
//! | `std` | Forwards `std` to whichever of the above are enabled |
//!
//! Serialization is the raw integer, not a decimal string: it round-trips
//! exactly, stays stable across versions, and does not invite a reader to think
//! the value is a float.
//!
//! ## Vector math libraries
//!
//! `nalgebra` needs nothing from this crate. Its blanket `Scalar` implementation
//! covers any `Clone + PartialEq + Debug + Any` type, so `Vector3<I24F8>` works
//! as-is, along with anything built from the operator traits — addition,
//! subtraction, scaling. `RealField` and `ComplexField` are deliberately not
//! implemented: they require `exp`, `ln`, and `powf` on the scalar, which a
//! fixed-point type cannot answer honestly, and implementing them would make
//! `norm()` compile and then misbehave.
//!
//! `mint` is a set of vector and matrix structs with no scalar traits, so a
//! scalar crate has nothing to interoperate with. Both belong with the vector
//! types in `corvid_transform`.

#![no_std]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    reason = "narrowing is this crate's subject matter; every cast is preceded by a range check, a saturating conversion, or a mask, and the exhaustive tests cover the boundaries"
)]

// `arbitrary`'s derive macro emits `::std` paths, so that feature — and only
// that feature — pulls in std. Nothing else here reaches past `core`.
#[cfg(feature = "arbitrary")]
extern crate std;

mod fixed;
mod trig;

pub use fixed::{angle, factor, pitch, point, signed};

pub use angle::{Angle8, Angle16, Angle32};
pub use factor::{Factor8, Factor16, Factor32};
pub use pitch::{Pitch8, Pitch16, Pitch32};
pub use point::{I0F8, I8F8, I24F8};
pub use signed::{Signed8, Signed16, Signed32};
