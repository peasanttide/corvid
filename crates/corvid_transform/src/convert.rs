//! World <-> local conversions, and the two tier conversions.
//!
//! # The hot path
//!
//! Order inside every `to_*` is **widen -> subtract -> range-check -> narrow ->
//! rotate**:
//!
//! 1. Widen the argument to `I48F16` -- an exact shift (`<< 8` from a
//!    [`GlobalPoint`], nothing at all from a [`GlobalFinePoint`]).
//! 2. Subtract the camera position -- exact `i64`, no rounding.
//! 3. Range-check and narrow. **This is the only failure point and the only
//!    source of `None`.**
//! 4. Rotate by the transposed basis, entirely in `i32 x i32 -> i64`.
//!
//! Steps 1-3 into a [`FinePoint`] are **bit-exact end to end**: shared 16-bit
//! fractions mean the widen is a shift, the subtract is exact `i64`, and the
//! narrow is a bounds test plus `as i32`. Nothing rounds until the rotation in
//! step 4, which rounds once. Every unit of error in a world->eye conversion is
//! attributable to one place.
//!
//! Narrowing a *difference* rather than an absolute is what makes earth scale
//! work: the camera can sit 6.37e6 m from the origin -- or 1e13 m -- and
//! near-field geometry still resolves to 15.26 um. **There is no `i128` on this
//! path.**

use corvid_fixed::I48F16;
use corvid_rotation::FineRotation;
use corvid_vector::{Direction, FinePoint, GlobalFinePoint, GlobalPoint};

use crate::{FineTransform, Transform};

/// Generates the conversion family for one transform tier.
macro_rules! impl_conversions {
    ($name:ident) => {
        impl $name {
            /// The offset from this transform's origin to `p`, at world width.
            ///
            /// Exact: both operands are already `I48F16`, so this is an `i64`
            /// subtraction and nothing else. `None` only when the two points
            /// are further apart than `I48F16` itself can express, which
            /// certainly means the offset does not fit any narrower type
            /// either.
            #[must_use]
            #[inline]
            const fn offset_to(self, p: GlobalFinePoint) -> Option<GlobalFinePoint> {
                p.checked_sub(self.origin())
            }

            /// World -> eye, at 15.26 um over +/-32.7 km. **The hot path.**
            ///
            /// Widen, subtract, range-check, narrow, rotate -- with nothing
            /// rounding until the rotation. Returns `None` when the offset
            /// leaves [`FinePoint`]'s +/-32.7 km, **before or after the
            /// rotation**: a rotation can map a corner of the cube onto an
            /// axis and make an in-range offset up to `sqrt(3) x` longer, and
            /// reporting a silently clamped position as `Some` would put
            /// near-field geometry kilometres from where it belongs.
            ///
            /// Use this for anything the eye renders;
            /// [`to_local`](Self::to_local) has the wider range and the coarser
            /// 3.9 mm resolution.
            ///
            /// # Hoisting
            ///
            /// This decodes the packed rotation on every call. A loop over
            /// thousands of points should call [`basis`](Self::basis) once and
            /// use [`Basis::unrotate_fine`](corvid_rotation::Basis::unrotate_fine)
            /// directly; `examples/earth_scale_vr.rs` measures what that saves.
            #[must_use]
            #[inline]
            pub const fn to_fine_global(self, p: GlobalFinePoint) -> Option<FinePoint> {
                let Some(offset) = self.offset_to(p) else {
                    return None;
                };
                let Some(near) = offset.to_fine() else {
                    return None;
                };
                self.basis().checked_unrotate_fine(near)
            }

            /// World -> eye, from an object-scale position.
            #[must_use]
            #[inline]
            pub const fn to_fine(self, p: GlobalPoint) -> Option<FinePoint> {
                self.to_fine_global(p.to_global_fine())
            }

            /// World -> local at object scale: 3.9 mm over +/-8388 km.
            #[must_use]
            #[inline]
            pub const fn to_local_global(self, p: GlobalFinePoint) -> Option<GlobalPoint> {
                let Some(offset) = self.offset_to(p) else {
                    return None;
                };
                let Some(coarse) = offset.to_global() else {
                    return None;
                };
                // Checked for the same reason `to_fine_global` is: the rotation
                // can make an in-range offset up to `sqrt(3) x` longer.
                self.basis().checked_unrotate_global(coarse)
            }

            /// World -> local at object scale, from an object-scale position.
            #[must_use]
            #[inline]
            pub const fn to_local(self, p: GlobalPoint) -> Option<GlobalPoint> {
                self.to_local_global(p.to_global_fine())
            }

            /// Eye -> world. Total.
            ///
            /// Stays on the `i64` path: the rotation accumulates at
            /// `i32 x i32 -> i64` and widens its *output* rather than its input,
            /// so a near-field offset that a rotation makes up to `sqrt(3) x` longer
            /// still lands exactly.
            #[must_use]
            #[inline]
            pub const fn to_world(self, v: FinePoint) -> GlobalFinePoint {
                self.basis().rotate_fine_wide(v).add(self.origin())
            }

            /// Local -> world, from an object-scale offset. Total.
            ///
            /// The `i128` path, because the operand is already world-scale.
            #[must_use]
            #[inline]
            pub const fn to_world_coarse(self, v: GlobalPoint) -> GlobalFinePoint {
                self.basis()
                    .rotate_global_fine(v.to_global_fine())
                    .add(self.origin())
            }

            /// Rotates a near-field offset without translating it.
            #[must_use]
            #[inline]
            pub const fn transform_vector(self, v: FinePoint) -> FinePoint {
                self.basis().rotate_fine(v)
            }

            /// The inverse rotation of a near-field offset.
            #[must_use]
            #[inline]
            pub const fn inverse_transform_vector(self, v: FinePoint) -> FinePoint {
                self.basis().unrotate_fine(v)
            }

            /// Rotates a unit direction into world space.
            #[must_use]
            #[inline]
            pub const fn transform_direction(self, d: Direction) -> Direction {
                self.basis().rotate_direction(d)
            }

            /// Rotates a unit direction back into local space.
            #[must_use]
            #[inline]
            pub const fn inverse_transform_direction(self, d: Direction) -> Direction {
                self.basis().unrotate_direction(d)
            }
        }
    };
}

impl_conversions!(Transform);
impl_conversions!(FineTransform);

impl Transform {
    /// Upgrades to the fine tier. Total.
    ///
    /// **Not lossless in the way the name suggests.** The position widens
    /// exactly -- `I24F8` to `I48F16` is a `<< 8` -- but the rotation is
    /// *re-quantized*, adding up to [`FineRotation`]'s 0.0033 deg on top of the
    /// 0.186 deg the [`Rotation`](corvid_rotation::Rotation) already carries. That is a 1.8% increase in a
    /// quantity already dominated by the coarse codec, not a free upgrade.
    #[must_use]
    #[inline]
    pub const fn to_fine_transform(self) -> FineTransform {
        FineTransform::new(
            self.position().to_global_fine(),
            FineRotation::from_rotation(self.rotation()),
        )
    }

    /// Rotates and translates a point from local space into world space.
    ///
    /// Saturates at [`GlobalPoint`]'s range rather than failing, which only
    /// bites for a point already near the edge of the world.
    #[must_use]
    #[inline]
    pub const fn transform_point(self, p: GlobalPoint) -> GlobalPoint {
        Self::narrow_saturating(self.to_world_coarse(p))
    }

    /// The inverse of [`transform_point`](Self::transform_point), or `None` if
    /// the point is out of local range.
    #[must_use]
    #[inline]
    pub const fn inverse_transform_point(self, p: GlobalPoint) -> Option<GlobalPoint> {
        self.to_local(p)
    }
}

impl FineTransform {
    /// Downgrades to the coarse tier, or `None` if the position does not fit.
    ///
    /// `None` comes **only** from position range -- `GlobalFinePoint`'s
    /// +/-1.407e14 m against `GlobalPoint`'s +/-8388 km. The rotation always
    /// converts, losing accuracy down to the 32-bit tier's 0.186 deg.
    #[must_use]
    #[inline]
    pub const fn to_coarse_transform(self) -> Option<Transform> {
        match self.position().to_global() {
            Some(position) => Some(Transform::new(position, self.rotation().to_rotation())),
            None => None,
        }
    }

    /// Rotates and translates a point from local space into world space.
    #[must_use]
    #[inline]
    pub const fn transform_point(self, p: GlobalFinePoint) -> GlobalFinePoint {
        self.basis().rotate_global_fine(p).add(self.origin())
    }

    /// The inverse of [`transform_point`](Self::transform_point), or `None` if
    /// the point is out of local range.
    #[must_use]
    #[inline]
    pub const fn inverse_transform_point(self, p: GlobalFinePoint) -> Option<GlobalFinePoint> {
        let Some(offset) = self.offset_to(p) else {
            return None;
        };
        // Checked for the same reason `Transform::to_local` is: the rotation
        // can make an in-range offset up to `sqrt(3) x` longer. The saturating form
        // would answer `Some` with a clamped axis, which reads as a position
        // rather than as the failure it is.
        self.basis().checked_unrotate_global_fine(offset)
    }
}

impl From<Transform> for FineTransform {
    #[inline]
    fn from(t: Transform) -> Self {
        t.to_fine_transform()
    }
}

/// The error from narrowing a [`FineTransform`] to a [`Transform`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PositionOutOfRange;

impl core::fmt::Display for PositionOutOfRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("position is outside GlobalPoint's range")
    }
}

impl TryFrom<FineTransform> for Transform {
    type Error = PositionOutOfRange;

    #[inline]
    fn try_from(t: FineTransform) -> Result<Self, Self::Error> {
        t.to_coarse_transform().ok_or(PositionOutOfRange)
    }
}

/// A last-bit sanity check that both tiers really do widen exactly.
#[allow(dead_code, reason = "asserted at compile time by the const evaluator")]
const _: () = {
    let coarse = GlobalPoint::new(
        corvid_fixed::I24F8::from_bits(1),
        corvid_fixed::I24F8::from_bits(-1),
        corvid_fixed::I24F8::from_bits(0),
    );
    let wide = coarse.to_global_fine();
    assert!(wide.x().to_bits() == 256);
    assert!(wide.y().to_bits() == -256);
    let _ = I48F16::ONE;
};
