//! Applying a versor: rotating a vector, reading an axis back, converting to
//! and from the matrix form, and interpolating between two rotations.

use corvid_fixed::{Angle32, Factor32};
use corvid_vector::{Direction, FinePoint, GlobalPoint};

use super::{ONE, SLERP_FALLBACK, Versor};
use crate::basis::{Basis, clamp_q30, entry, round_shift_i64, signed_from_q30};
use crate::normalize::normalize4;

impl Versor {
    /// Rotates a near-field offset, by way of the matrix form.
    ///
    /// Roughly three times a [`Basis`]'s cost per point: this builds the whole
    /// matrix and then throws it away. Converting once with
    /// [`to_basis`](Self::to_basis) and using [`Basis::rotate_fine`] is what a
    /// loop over many points should do.
    #[must_use]
    #[inline]
    pub const fn rotate_fine(self, v: FinePoint) -> FinePoint {
        self.to_basis().rotate_fine(v)
    }

    /// The inverse rotation of a near-field offset.
    #[must_use]
    #[inline]
    pub const fn unrotate_fine(self, v: FinePoint) -> FinePoint {
        self.to_basis().unrotate_fine(v)
    }

    /// Rotates an object-scale offset, by way of the matrix form.
    #[must_use]
    #[inline]
    pub const fn rotate_global(self, v: GlobalPoint) -> GlobalPoint {
        self.to_basis().rotate_global(v)
    }

    /// The inverse rotation of an object-scale offset.
    #[must_use]
    #[inline]
    pub const fn unrotate_global(self, v: GlobalPoint) -> GlobalPoint {
        self.to_basis().unrotate_global(v)
    }

    /// Rotates a unit direction, by way of the matrix form.
    #[must_use]
    #[inline]
    pub const fn rotate_direction(self, d: Direction) -> Direction {
        self.to_basis().rotate_direction(d)
    }

    /// The inverse rotation of a unit direction.
    #[must_use]
    #[inline]
    pub const fn unrotate_direction(self, d: Direction) -> Direction {
        self.to_basis().unrotate_direction(d)
    }

    /// The local **+Y** axis in world space: forward.
    #[must_use]
    #[inline]
    pub const fn forward(self) -> Direction {
        self.to_basis().forward()
    }

    /// The local **+X** axis in world space: rightward.
    #[must_use]
    #[inline]
    pub const fn right(self) -> Direction {
        self.to_basis().right()
    }

    /// The local **+Z** axis in world space: upward.
    #[must_use]
    #[inline]
    pub const fn up(self) -> Direction {
        self.to_basis().up()
    }

    /// The matrix form of this rotation.
    ///
    /// Every entry lands in `[-1, 1]`, comfortably inside [`I2F30`](corvid_fixed::I2F30), and each
    /// rounds once from a Q60 intermediate.
    #[must_use]
    #[inline]
    pub const fn to_basis(self) -> Basis {
        let q = self.bits();
        let [x, y, z, w] = q;

        // Every product below is Q60; the shift takes them back to Q30.
        let xx = x * x;
        let yy = y * y;
        let zz = z * z;
        let xy = x * y;
        let xz = x * z;
        let yz = y * z;
        let wx = w * x;
        let wy = w * y;
        let wz = w * z;
        let one = ONE << 30;

        Basis::from_rows_unchecked([
            [
                entry(one - 2 * (yy + zz)),
                entry(2 * (xy - wz)),
                entry(2 * (xz + wy)),
            ],
            [
                entry(2 * (xy + wz)),
                entry(one - 2 * (xx + zz)),
                entry(2 * (yz - wx)),
            ],
            [
                entry(2 * (xz - wy)),
                entry(2 * (yz + wx)),
                entry(one - 2 * (xx + yy)),
            ],
        ])
    }

    /// The versor form of a rotation matrix.
    ///
    /// Branches on the largest of the four candidate denominators -- the same
    /// chart idea the 32-bit codec uses -- so the division is never by a value
    /// near zero. The `normalize4` at the end absorbs the rounding.
    #[must_use]
    #[inline]
    pub const fn from_basis(m: Basis) -> Self {
        let r = m.to_rows();
        let m00 = r[0][0].to_bits() as i64;
        let m01 = r[0][1].to_bits() as i64;
        let m02 = r[0][2].to_bits() as i64;
        let m10 = r[1][0].to_bits() as i64;
        let m11 = r[1][1].to_bits() as i64;
        let m12 = r[1][2].to_bits() as i64;
        let m20 = r[2][0].to_bits() as i64;
        let m21 = r[2][1].to_bits() as i64;
        let m22 = r[2][2].to_bits() as i64;

        // The four candidates are `4w^2`, `4x^2`, `4y^2`, `4z^2` minus one, at Q30.
        // Whichever is largest names the component that is safely far from
        // zero, and the other three follow from the off-diagonal sums. Because
        // `normalize4` only cares about ratios, the four expressions below need
        // no common scale factor at all.
        let trace = m00 + m11 + m22;
        if trace > 0 {
            Self::from_xyzw_unchecked(normalize4([m21 - m12, m02 - m20, m10 - m01, ONE + trace]))
        } else if m00 > m11 && m00 > m22 {
            Self::from_xyzw_unchecked(normalize4([
                ONE + m00 - m11 - m22,
                m01 + m10,
                m02 + m20,
                m21 - m12,
            ]))
        } else if m11 > m22 {
            Self::from_xyzw_unchecked(normalize4([
                m01 + m10,
                ONE - m00 + m11 - m22,
                m12 + m21,
                m02 - m20,
            ]))
        } else {
            Self::from_xyzw_unchecked(normalize4([
                m02 + m20,
                m12 + m21,
                ONE - m00 - m11 + m22,
                m10 - m01,
            ]))
        }
    }

    /// Normalized linear interpolation: **the default**.
    ///
    /// A component-wise lerp followed by one renormalize. Its departure from
    /// constant angular velocity is not observable over the few degrees a frame
    /// actually spans, and it costs one `rsqrt` where [`slerp`](Self::slerp)
    /// costs an `acos` and two `sin`s -- the two slowest functions in
    /// `corvid_fixed`.
    ///
    /// Interpolates along the short way round: if the two versors are more than
    /// a half turn apart on the double cover, `to` is negated first.
    #[must_use]
    #[inline]
    pub const fn nlerp(self, to: Self, weight: Factor32) -> Self {
        let to = if self.dot(to).is_negative() {
            to.negate()
        } else {
            to
        };
        self.nlerp_canonical(to, weight)
    }

    /// [`nlerp`](Self::nlerp) with `to` already on the near side of the double
    /// cover, so the caller's own dot product is not computed a second time.
    #[inline]
    const fn nlerp_canonical(self, to: Self, weight: Factor32) -> Self {
        let (a, b) = (self.bits(), to.bits());
        let t = weight.to_bits() as i64;
        let full = Factor32::MAX.to_bits() as i64;

        // Half of the `Factor32::MAX` denominator, for the rounding below.
        let half = full / 2;

        let mut mixed = [0i64; 4];
        let mut i = 0;
        while i < 4 {
            // `a + (b - a) * t`, with the division by `Factor32::MAX` folded
            // in and rounded **half away from zero**, as every other reduction
            // in the crate is. A truncating divide would pull each component
            // toward `self`, so `a.nlerp(b, t)` and `b.nlerp(a, MAX - t)` would
            // name different rotations and the bias would accumulate over a
            // chain rather than cancel.
            //
            // Unit components bound `|b - a|` by `2^31` and `t` by `2^32 - 1`,
            // so `|delta| + half` reaches exactly `i64::MAX` and no further.
            let delta = (b[i] - a[i]) * t;
            let scaled = if delta >= 0 {
                (delta + half) / full
            } else {
                -((-delta + half) / full)
            };
            mixed[i] = a[i] + scaled;
            i += 1;
        }
        Self::from_xyzw_unchecked(normalize4(mixed))
    }

    /// Spherical linear interpolation: constant angular velocity, at a price.
    ///
    /// Needs an `acos` and two `sin`s, all on `corvid_fixed`'s CORDIC path,
    /// which is where the crate's slowest functions live. Reach for it only
    /// when constant angular velocity genuinely matters;
    /// [`nlerp`](Self::nlerp) is what per-frame interpolation should use.
    ///
    /// Falls back to `nlerp` when the two rotations are within a whisker of
    /// each other, where the sines underflow and the formula loses all its
    /// precision.
    #[must_use]
    #[inline]
    pub const fn slerp(self, to: Self, weight: Factor32) -> Self {
        // One dot serves both the sign test and the cosine. `negate` is exact
        // on unit components and the rounding in `dot` is symmetric, so the dot
        // against the negated versor is exactly this one negated.
        let signed = self.dot(to).to_bits() as i64;
        let to = if signed < 0 { to.negate() } else { to };
        self.slerp_canonical(to, weight, clamp_q30(signed.abs()))
    }

    /// [`slerp`](Self::slerp) with `to` already on the near side of the double
    /// cover and `cosine` the non-negative dot product between them.
    ///
    /// Both public entry points already hold those two values, and `acos` is
    /// the slowest function the crate has -- [`rotate_towards`](Self::rotate_towards)
    /// paid for a second one, a third of its cost, before this existed.
    #[inline]
    pub(crate) const fn slerp_canonical(self, to: Self, weight: Factor32, cosine: i64) -> Self {
        // Within about a thousandth of a turn the sines underflow; nlerp and
        // slerp agree to well under a last bit there anyway.
        if cosine >= ONE - SLERP_FALLBACK {
            return self.nlerp_canonical(to, weight);
        }

        let theta = Angle32::acos(signed_from_q30(cosine as i32));
        let sin_theta = theta.sin().to_bits() as i64;
        if sin_theta == 0 {
            return self.nlerp_canonical(to, weight);
        }

        // The two weights, `sin((1-t)theta)/sin(theta)` and `sin(ttheta)/sin(theta)`.
        let t = weight.to_bits() as u64;
        let full = Factor32::MAX.to_bits() as u64;
        let scaled = ((theta.to_bits() as u64) * t / full) as u32;
        let from_weight = Angle32::from_bits(theta.to_bits() - scaled).sin().to_bits() as i64;
        let to_weight = Angle32::from_bits(scaled).sin().to_bits() as i64;

        let (a, b) = (self.bits(), to.bits());
        let mut mixed = [0i64; 4];
        let mut i = 0;
        while i < 4 {
            // Scale-free: `normalize4` divides the common `1/sin(theta)` out.
            mixed[i] = round_shift_i64(a[i] * from_weight + b[i] * to_weight, 31);
            i += 1;
        }
        Self::from_xyzw_unchecked(normalize4(mixed))
    }
}
