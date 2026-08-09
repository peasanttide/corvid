//! The optional `mint` and `nalgebra` integrations.
//!
//! Both are boundary conversions and therefore the one place in this crate
//! where floating point appears at all. They go through `f64`, so the
//! conversion rounds once.

use crate::{Direction, FinePoint, GlobalFinePoint, GlobalPoint};

/// Implements the outgoing float-vector conversions for one point type.
macro_rules! impl_interop_out {
    ($name:ident) => {
        #[cfg(feature = "mint")]
        impl From<$name> for ::mint::Vector3<f64> {
            #[inline]
            fn from(point: $name) -> Self {
                Self {
                    x: point.x().to_f64(),
                    y: point.y().to_f64(),
                    z: point.z().to_f64(),
                }
            }
        }

        #[cfg(feature = "mint")]
        impl From<$name> for ::mint::Vector3<f32> {
            #[inline]
            fn from(point: $name) -> Self {
                Self {
                    x: point.x().to_f32(),
                    y: point.y().to_f32(),
                    z: point.z().to_f32(),
                }
            }
        }

        #[cfg(feature = "nalgebra")]
        impl From<$name> for ::nalgebra::Vector3<f64> {
            #[inline]
            fn from(point: $name) -> Self {
                Self::new(point.x().to_f64(), point.y().to_f64(), point.z().to_f64())
            }
        }
    };
}

/// Implements the incoming float-vector conversions for one *position* type,
/// where a component is a length and clamping it is the honest answer.
macro_rules! impl_interop_in {
    ($name:ident, $scalar:ident) => {
        #[cfg(feature = "mint")]
        impl From<::mint::Vector3<f64>> for $name {
            #[inline]
            fn from(vector: ::mint::Vector3<f64>) -> Self {
                Self::new(
                    ::corvid_fixed::$scalar::from_f64(vector.x),
                    ::corvid_fixed::$scalar::from_f64(vector.y),
                    ::corvid_fixed::$scalar::from_f64(vector.z),
                )
            }
        }

        #[cfg(feature = "mint")]
        impl From<::mint::Vector3<f32>> for $name {
            #[inline]
            fn from(vector: ::mint::Vector3<f32>) -> Self {
                Self::new(
                    ::corvid_fixed::$scalar::from_f32(vector.x),
                    ::corvid_fixed::$scalar::from_f32(vector.y),
                    ::corvid_fixed::$scalar::from_f32(vector.z),
                )
            }
        }

        #[cfg(feature = "nalgebra")]
        impl From<::nalgebra::Vector3<f64>> for $name {
            #[inline]
            fn from(vector: ::nalgebra::Vector3<f64>) -> Self {
                Self::new(
                    ::corvid_fixed::$scalar::from_f64(vector.x),
                    ::corvid_fixed::$scalar::from_f64(vector.y),
                    ::corvid_fixed::$scalar::from_f64(vector.z),
                )
            }
        }
    };
}

impl_interop_out!(GlobalFinePoint);
impl_interop_out!(GlobalPoint);
impl_interop_out!(FinePoint);
impl_interop_out!(Direction);

impl_interop_in!(GlobalFinePoint, I48F16);
impl_interop_in!(GlobalPoint, I24F8);
impl_interop_in!(FinePoint, I16F16);

/// A [`Direction`] from three floats, **normalized** rather than clamped.
///
/// A `Direction` denotes a unit direction, so the component-wise
/// `Signed32::from_f64` the position types use is wrong here: it clamps each
/// axis independently, and `(3, 4, 0)` -- an ordinary unnormalized direction
/// from an engine boundary -- would arrive as `(1, 1, 0)`, 8 deg off. Only the
/// ratios matter, so the vector is rescaled onto the `Signed32` range first
/// and the crate's own integer normalize finishes the job.
///
/// A zero or non-finite vector names no direction; **+Y**, the crate's forward,
/// is returned for it.
#[cfg(any(feature = "mint", feature = "nalgebra"))]
fn direction_from_f64(x: f64, y: f64, z: f64) -> Direction {
    use corvid_fixed::Signed32;

    /// `Signed32::MAX`'s bit pattern, as the scale to rescale onto.
    const SCALE: f64 = 2_147_483_647.0;
    /// The answer when there is no direction to name.
    const FORWARD: Direction = Direction::new(Signed32::ZERO, Signed32::MAX, Signed32::ZERO);

    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return FORWARD;
    }
    let largest = x.abs().max(y.abs()).max(z.abs());
    if largest <= 0.0 {
        return FORWARD;
    }
    let factor = SCALE / largest;
    // `as i32` saturates in Rust, and the rescale keeps every component inside
    // the range anyway, so no component can wrap.
    let component = |v: f64| Signed32::from_bits((v * factor) as i32);
    Direction::new(component(x), component(y), component(z))
        .normalize()
        .unwrap_or(FORWARD)
}

#[cfg(feature = "mint")]
impl From<::mint::Vector3<f64>> for Direction {
    #[inline]
    fn from(vector: ::mint::Vector3<f64>) -> Self {
        direction_from_f64(vector.x, vector.y, vector.z)
    }
}

#[cfg(feature = "mint")]
impl From<::mint::Vector3<f32>> for Direction {
    #[inline]
    fn from(vector: ::mint::Vector3<f32>) -> Self {
        direction_from_f64(
            f64::from(vector.x),
            f64::from(vector.y),
            f64::from(vector.z),
        )
    }
}

#[cfg(feature = "nalgebra")]
impl From<::nalgebra::Vector3<f64>> for Direction {
    #[inline]
    fn from(vector: ::nalgebra::Vector3<f64>) -> Self {
        direction_from_f64(vector.x, vector.y, vector.z)
    }
}
