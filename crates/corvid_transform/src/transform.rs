//! The two rigid transform types.
//!
//! Objects in the world are [`Transform`]. The camera and the VR tracked poses
//! are [`FineTransform`]. Both come from one macro, so the operation family is
//! written once and cannot drift between them.
//!
//! **Both widen to `I48F16` internally.** `Transform`'s own position is a
//! [`GlobalPoint`], so a naive implementation would subtract in `i32` and need
//! a separate code path with its own overflow story -- two `GlobalPoint`s can
//! differ by more than `GlobalPoint` holds. Widening the operands to `I48F16`
//! first is an exact `<< 8`, makes the subtraction total, and lets both tiers
//! share one macro body. The shift is free next to the rotation that follows.

use corvid_fixed::{I24F8, I48F16};
use corvid_rotation::{Basis, FineRotation, Rotation};
use corvid_vector::{Direction, GlobalFinePoint, GlobalPoint};

/// Generates a rigid transform over one position type and one packed rotation.
macro_rules! define_transform {
    (
        $(#[$attr:meta])*
        $name:ident {
            position: $position:ident,
            rotation: $rotation:ident,
            widen: $widen:ident,
        }
    ) => {
        $(#[$attr])*
        #[repr(C)]
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        #[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
        #[cfg_attr(feature = "arbitrary", derive(::arbitrary::Arbitrary))]
        #[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
        pub struct $name {
            position: $position,
            rotation: $rotation,
        }

        impl Default for $name {
            #[inline]
            fn default() -> Self {
                Self::IDENTITY
            }
        }

        impl $name {
            /// At the origin, facing **+Y** with **+Z** up.
            pub const IDENTITY: Self = Self {
                position: $position::ZERO,
                rotation: $rotation::IDENTITY,
            };

            /// Builds a transform from a position and a rotation.
            #[must_use]
            #[inline]
            pub const fn new(position: $position, rotation: $rotation) -> Self {
                Self { position, rotation }
            }

            /// The position in world space.
            #[must_use]
            #[inline]
            pub const fn position(self) -> $position {
                self.position
            }

            /// The packed rotation.
            #[must_use]
            #[inline]
            pub const fn rotation(self) -> $rotation {
                self.rotation
            }

            /// The rotation as a matrix, decoded.
            ///
            /// **Hoist this out of hot loops.** Every world<->local conversion on
            /// this type decodes the packed rotation on the way in, and that
            /// decode is the dominant cost. A loop over thousands of points
            /// should call `basis` once and use [`Basis`]'s own rotate and
            /// unrotate; `examples/earth_scale_vr.rs` measures the difference.
            #[must_use]
            #[inline]
            pub const fn basis(self) -> Basis {
                self.rotation.to_basis()
            }

            /// The position, widened to the common `I48F16` working type.
            ///
            /// Exact: from a [`GlobalPoint`] it is a `<< 8`, and from a
            /// [`GlobalFinePoint`] it is nothing at all.
            #[must_use]
            #[inline]
            pub const fn origin(self) -> GlobalFinePoint {
                self.position.$widen()
            }

            /// The same transform at a different position.
            #[must_use]
            #[inline]
            pub const fn with_position(self, position: $position) -> Self {
                Self { position, rotation: self.rotation }
            }

            /// The same transform with a different rotation.
            #[must_use]
            #[inline]
            pub const fn with_rotation(self, rotation: $rotation) -> Self {
                Self { position: self.position, rotation }
            }

            /// Moved by a world-space offset, saturating at the position
            /// type's range.
            #[must_use]
            #[inline]
            pub const fn translated_by(self, offset: $position) -> Self {
                Self { position: self.position.add(offset), rotation: self.rotation }
            }

            /// Turned by a further rotation, applied **after** this one.
            ///
            /// Composed in versor form: sixteen multiplies against a matrix's
            /// twenty-seven, and no matrix is built or taken apart again on
            /// either side of it.
            #[must_use]
            #[inline]
            pub const fn rotated_by(self, rotation: $rotation) -> Self {
                Self {
                    position: self.position,
                    rotation: $rotation::from_versor(
                        rotation.to_versor().compose(self.rotation.to_versor()),
                    ),
                }
            }

            /// The local **+Y** axis in world space: forward.
            #[must_use]
            #[inline]
            pub const fn forward(self) -> Direction {
                self.basis().forward()
            }

            /// The local **+X** axis in world space: rightward.
            #[must_use]
            #[inline]
            pub const fn right(self) -> Direction {
                self.basis().right()
            }

            /// The local **+Z** axis in world space: upward.
            #[must_use]
            #[inline]
            pub const fn up(self) -> Direction {
                self.basis().up()
            }

            /// Composes two transforms, applying `rhs` **first**, then `self`.
            ///
            /// Matrix multiplication order, and `glam`'s `Mul`. Covered by a
            /// test that fails if the order is ever flipped.
            #[must_use]
            #[inline]
            pub const fn compose(self, rhs: Self) -> Self {
                // One decode of each packed rotation. The position needs
                // `self`'s matrix; the rotation composes in versor form, so
                // `rhs`'s matrix is never built and the result is never taken
                // apart into one again.
                let q = self.rotation.to_versor();
                // `self` applied to `rhs`'s position, then offset by `self`'s.
                let moved = q.to_basis().rotate_global_fine(rhs.origin());
                Self {
                    position: Self::narrow_saturating(moved.add(self.origin())),
                    rotation: $rotation::from_versor(q.compose(rhs.rotation.to_versor())),
                }
            }

            /// The transform that undoes this one.
            ///
            /// The inverse rotation is the transpose, so it is exact up to the
            /// packed codec's own re-quantization. The inverse *position* is
            /// `-R^-1t`, whose length equals `|t|` -- so it **saturates** for a
            /// transform whose position is longer than the position type holds
            /// along the rotated axis, even though the original was
            /// representable.
            ///
            /// # Accuracy far from the origin
            ///
            /// The stored rotation is quantized, so `t.inverse().compose(t)`
            /// leaves a position residual of roughly `|t| x quantum`. For
            /// [`Transform`], whose 0.186 deg is 3.2e-3 radians, that is tens of
            /// kilometres at 8000 km out. This is why the camera is a
            /// [`FineTransform`]: its quantum is 55x smaller *and* its position
            /// resolution 256x finer.
            #[must_use]
            #[inline]
            pub const fn inverse(self) -> Self {
                // For a unit versor the inverse is the conjugate, and the
                // matrix of the conjugate is exactly the transpose -- so one
                // decode serves both, and the result never round-trips back
                // through a matrix.
                let q = self.rotation.to_versor();
                let moved = q.to_basis().inverse().rotate_global_fine(self.origin());
                Self {
                    position: Self::narrow_saturating(moved.neg()),
                    rotation: $rotation::from_versor(q.inverse()),
                }
            }
        }
    };
}

define_transform! {
    /// An object in the world: a [`GlobalPoint`] and a [`Rotation`], **16
    /// bytes**.
    ///
    /// | | |
    /// |---|---|
    /// | Position | +/-8388 km at 3.9 mm |
    /// | Rotation | 0.1856 deg worst case |
    ///
    /// This is what ordinary objects use. The camera and any VR tracked pose
    /// want [`FineTransform`] instead.
    ///
    /// # A zeroed `Transform` is not the identity
    ///
    /// Every `u32` names a rotation, and the all-zero one names a 120 deg turn
    /// about `(-1, 1, 1)` -- the chart-`x` Gibbs vector `(-1, -1, -1)`. So
    /// `bytemuck::zeroed()`, a calloc'd scene buffer, or a zero-filled network
    /// packet gives an object that is *rotated*, where the same idiom on a
    /// [`FineTransform`] gives the identity. Use
    /// [`IDENTITY`](Self::IDENTITY) or [`Default`] to mean "unrotated".
    Transform {
        position: GlobalPoint,
        rotation: Rotation,
        widen: to_global_fine,
    }
}

define_transform! {
    /// A camera or VR tracked pose: a [`GlobalFinePoint`] and a
    /// [`FineRotation`], **32 bytes**.
    ///
    /// | | |
    /// |---|---|
    /// | Position | +/-1.407e14 m at 15.26 um |
    /// | Rotation | 0.0033 deg worst case |
    ///
    /// The extra width is what lets a camera sit 6.37e6 m -- or 1e13 m -- from
    /// the origin while near-field geometry still resolves to the last bit.
    FineTransform {
        position: GlobalFinePoint,
        rotation: FineRotation,
        widen: to_global_fine,
    }
}

impl Transform {
    /// Narrows a world-scale point back into this tier's position type,
    /// saturating.
    #[inline]
    pub(crate) const fn narrow_saturating(point: GlobalFinePoint) -> GlobalPoint {
        if let Some(narrowed) = point.to_global() {
            narrowed
        } else {
            // Past `GlobalPoint`'s range on at least one axis. Clamp each axis
            // independently, which is what the point type's own saturating
            // arithmetic would do.
            let [x, y, z] = point.to_array();
            GlobalPoint::new(saturate_global(x), saturate_global(y), saturate_global(z))
        }
    }
}

impl FineTransform {
    /// The identity narrowing: this tier already works at `I48F16`.
    #[inline]
    pub(crate) const fn narrow_saturating(point: GlobalFinePoint) -> GlobalFinePoint {
        point
    }
}

/// Clamps one `I48F16` component into `I24F8`.
#[inline]
const fn saturate_global(value: I48F16) -> I24F8 {
    let bits = value.to_bits();
    // Round the eight fractional bits away first, then clamp. The rounding runs
    // on the unsigned magnitude: this is reached precisely when a component has
    // saturated, so `bits` can be `i64::MAX` and `bits + half` would overflow.
    let scaled = ((bits.unsigned_abs() + (1u64 << 7)) >> 8) as i64;
    let scaled = if bits >= 0 { scaled } else { -scaled };
    if scaled > I24F8::MAX.to_bits() as i64 {
        I24F8::MAX
    } else if scaled < I24F8::MIN.to_bits() as i64 {
        I24F8::MIN
    } else {
        I24F8::from_bits(scaled as i32)
    }
}

impl core::fmt::Debug for Transform {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Transform({:?}, {:?})", self.position, self.rotation)
    }
}

impl core::fmt::Debug for FineTransform {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "FineTransform({:?}, {:?})", self.position, self.rotation)
    }
}
