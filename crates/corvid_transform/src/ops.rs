//! The game-dev operation family: aiming, interpolating and stepping.
//!
//! `Option` appears only on genuinely degenerate cases -- `look_at` with forward
//! parallel to up, `direction_to` on coincident points, and the range-checked
//! narrowings. Everything else is total, as the workspace's `panic = "deny"`
//! requires.
//!
//! There are no view or projection matrices here. That is the renderer's
//! concern.

use corvid_fixed::{Angle32, Factor32, I24F8, I48F16};
use corvid_rotation::{Basis, FineRotation, Rotation};
use corvid_vector::{Direction, GlobalFinePoint, GlobalPoint};

use crate::{FineTransform, Transform};

/// Generates the operation family for one transform tier.
macro_rules! impl_ops {
    ($name:ident, $position:ident, $scalar:ident, $rotation:ident, $scale:expr) => {
        impl $name {
            /// How many `I48F16` bits one bit of this tier's position scalar is
            /// worth, for arithmetic done at the wide working scale.
            const POSITION_SCALE: i128 = $scale;

            /// A transform at `eye` looking at `target`, with `up` overhead.
            ///
            /// Returns `None` when the direction to the target is parallel to
            /// `up`, when `up` is zero-length, or when `eye` and `target`
            /// coincide -- all three being cases where there is no rotation to
            /// name rather than a rotation we decline to compute.
            #[must_use]
            #[inline]
            pub const fn look_at(eye: $position, target: $position, up: Direction) -> Option<Self> {
                // `direction_to` subtracts at full width. Taking the
                // difference in a position type and normalizing that clamps
                // each axis independently, which does not preserve a bearing:
                // two points 8388 km apart in x and 4194 km in y would come
                // back as a 45 deg heading. Widening to `I48F16` first fixes that
                // for this tier only -- `GlobalFinePoint` *is* the wide type
                // and has no headroom to widen into.
                match eye.to_global_fine().direction_to(target.to_global_fine()) {
                    Some(direction) => Self::looking_to(eye, direction, up),
                    None => None,
                }
            }

            /// A transform at `eye` looking along `forward`, with `up` overhead.
            #[must_use]
            #[inline]
            pub const fn looking_to(
                eye: $position,
                forward: Direction,
                up: Direction,
            ) -> Option<Self> {
                match Basis::look_to(forward, up) {
                    Some(basis) => Some(Self::new(eye, $rotation::from_basis(basis))),
                    None => None,
                }
            }

            /// This transform re-aimed at `target`, staying where it is.
            #[must_use]
            #[inline]
            pub const fn looking_at(self, target: $position, up: Direction) -> Option<Self> {
                Self::look_at(self.position(), target, up)
            }

            /// The unit direction from here to `target`, or `None` if the two
            /// coincide.
            #[must_use]
            #[inline]
            pub const fn direction_to(self, target: $position) -> Option<Direction> {
                // Subtracted at full width, for the same reason `look_at` is:
                // a saturating difference clamps each axis independently and
                // would report a bearing that is not the one to the target.
                self.origin().direction_to(target.to_global_fine())
            }

            /// The distance from here to `target`.
            #[must_use]
            #[inline]
            pub const fn distance_to(self, target: $position) -> $scalar {
                self.position().distance(target)
            }

            /// Interpolates position and rotation toward `to`.
            ///
            /// The position lerps and the rotation uses **`nlerp`**: over the
            /// few degrees a frame actually spans its departure from constant
            /// angular velocity is not observable, and it costs one `rsqrt`
            /// where `slerp` costs an `acos` and two `sin`s. Reach for
            /// [`Versor::slerp`](corvid_rotation::Versor::slerp) directly when
            /// constant angular velocity genuinely matters.
            ///
            /// Exact at both ends.
            #[must_use]
            #[inline]
            pub const fn lerp(self, to: Self, weight: Factor32) -> Self {
                Self::new(
                    self.position().lerp(to.position(), weight),
                    // Straight in and out of versor form. Going through
                    // `basis()` would decode each rotation to a matrix and
                    // rebuild a versor from it, then do it once more on the
                    // way out -- three `rsqrt` normalizes that cancel.
                    $rotation::from_versor(
                        self.rotation()
                            .to_versor()
                            .nlerp(to.rotation().to_versor(), weight),
                    ),
                )
            }

            /// Moves toward `target`'s position by at most `max_distance`,
            /// never overshooting.
            ///
            /// The rotation is left alone; [`rotate_towards`](Self::rotate_towards)
            /// is the other half.
            #[must_use]
            #[inline]
            pub const fn move_towards(self, target: Self, max_distance: $scalar) -> Self {
                if max_distance.is_negative() {
                    // Not a distance. Going on would divide a negative
                    // numerator and wrap it into a near-full weight through
                    // `as u32` -- a step of `-1 m` would travel almost the whole
                    // way. Staying put is the honest answer.
                    return self;
                }
                // Measured at `i128` rather than through any `distance`, all of
                // which narrow into a scalar and saturate at its `MAX`: a
                // fraction taken against a too-small denominator overshoots,
                // and `never overshoots` is this method's contract. Widening to
                // `I48F16` is enough for this tier's own positions but not for
                // `FineTransform`'s, whose positions already are `I48F16`.
                let remaining = wide_distance(target.origin(), self.origin());
                let step = (max_distance.to_bits() as i128) * Self::POSITION_SCALE;
                if remaining <= step || remaining == 0 {
                    return self.with_position(target.position());
                }
                // The fraction of the way to travel.
                let fraction = (step * (Factor32::MAX.to_bits() as i128) / remaining) as u32;
                self.with_position(
                    self.position()
                        .lerp(target.position(), Factor32::from_bits(fraction)),
                )
            }

            /// Turns toward `target`'s rotation by at most `max_step`, never
            /// overshooting.
            ///
            /// The position is left alone.
            #[must_use]
            #[inline]
            pub const fn rotate_towards(self, target: Self, max_step: Angle32) -> Self {
                self.with_rotation($rotation::from_versor(
                    self.rotation()
                        .to_versor()
                        .rotate_towards(target.rotation().to_versor(), max_step),
                ))
            }
        }
    };
}

/// The distance between two world-scale points, in `I48F16` bit units, at
/// `i128`.
///
/// [`GlobalFinePoint::distance`](corvid_vector::GlobalFinePoint::distance)
/// answers in `I48F16` and so saturates at `I48F16::MAX`, which opposite
/// corners of the world reach -- `sqrt(3) x 1.407e14` is past the type's own range.
/// `move_towards` divides by this, and a too-small denominator overshoots.
///
/// The component difference is exact at `i128`. Three squares must still fit
/// `u128`, so a difference wider than 62 bits is shifted down first and the
/// root shifted back -- the reduction is a power of two, so it costs at most a
/// last bit of a distance already past `1.4e14` m.
#[inline]
const fn wide_distance(a: GlobalFinePoint, b: GlobalFinePoint) -> i128 {
    let [ax, ay, az] = a.to_array();
    let [bx, by, bz] = b.to_array();
    let d = [
        (ax.to_bits() as i128 - bx.to_bits() as i128).unsigned_abs(),
        (ay.to_bits() as i128 - by.to_bits() as i128).unsigned_abs(),
        (az.to_bits() as i128 - bz.to_bits() as i128).unsigned_abs(),
    ];

    let mut largest = d[0];
    let mut i = 1;
    while i < 3 {
        if d[i] > largest {
            largest = d[i];
        }
        i += 1;
    }
    let bit_length = 128 - largest.leading_zeros();
    let shift = bit_length.saturating_sub(62);

    let (x, y, z) = (d[0] >> shift, d[1] >> shift, d[2] >> shift);
    let squared = x * x + y * y + z * z;
    let root = squared.isqrt();
    // Round up when the true root is past the halfway point, which happens
    // exactly when the remainder exceeds the root -- the rule `length` uses.
    let rounded = if squared - root * root > root {
        root + 1
    } else {
        root
    };
    (rounded << shift) as i128
}

// `I24F8` and `I48F16` differ by eight fractional bits; `I48F16` is the working
// scale itself.
impl_ops!(Transform, GlobalPoint, I24F8, Rotation, 256);
impl_ops!(FineTransform, GlobalFinePoint, I48F16, FineRotation, 1);
