//! The operation family on [`Basis`], which is where a rotation is read back
//! and applied.

use corvid_fixed::{Angle32, Pitch32};
use corvid_vector::Direction;

use super::{ONE, POLE_MARGIN, axis_entry, cross_normalized, mul3, q30_from_signed};
use crate::basis::{Basis, clamp_q30, entry, signed_from_q30};
use crate::versor::Versor;

impl Basis {
    /// The versor form, in a `const` context.
    ///
    /// `From` cannot be `const`, so the conversions this module needs go
    /// through here.
    #[must_use]
    #[inline]
    pub const fn to_versor_const(self) -> Versor {
        Versor::from_basis(self)
    }

    /// The rotation of `angle` about `axis`.
    #[must_use]
    #[inline]
    pub const fn from_axis_angle(axis: Direction, angle: Angle32) -> Self {
        Versor::from_axis_angle(axis, angle).to_basis()
    }

    /// The axis and angle this rotation turns through.
    #[must_use]
    #[inline]
    pub const fn to_axis_angle(self) -> (Direction, Angle32) {
        self.to_versor_const().to_axis_angle()
    }

    /// The rotation taking `from` onto `to`, along the shortest arc.
    #[must_use]
    #[inline]
    pub const fn from_rotation_arc(from: Direction, to: Direction) -> Self {
        Versor::from_rotation_arc(from, to).to_basis()
    }

    /// Builds a rotation from yaw, pitch and roll. **ZXY intrinsic**:
    /// `R = Rz(yaw) * Rx(pitch) * Ry(roll)`.
    ///
    /// Yaw is about **+Z**, pitch about **+X**, roll about **+Y**. All three
    /// zero gives [`IDENTITY`](Self::IDENTITY), which faces +Y with +Z up.
    #[must_use]
    #[inline]
    pub const fn from_yaw_pitch_roll(yaw: Angle32, pitch: Pitch32, roll: Angle32) -> Self {
        let (sin_yaw, cos_yaw) = yaw.sin_cos();
        let (sin_pitch, cos_pitch) = pitch.sin_cos();
        let (sin_roll, cos_roll) = roll.sin_cos();

        let (sy, cy) = (q30_from_signed(sin_yaw), q30_from_signed(cos_yaw));
        let (sp, cp) = (q30_from_signed(sin_pitch), q30_from_signed(cos_pitch));
        let (sr, cr) = (q30_from_signed(sin_roll), q30_from_signed(cos_roll));

        // Rz(yaw) * Rx(pitch) * Ry(roll), multiplied out. Every term is a
        // product of two or three Q30 values, shifted back after.
        //
        // A two-factor term is written as a plain product: `mul3(a, b, ONE)`
        // would round it to Q30 and shift it straight back to Q60, throwing
        // away thirty bits before the sum for nothing.
        let m00 = cy * cr - mul3(sy, sp, sr);
        let m01 = -(sy * cp);
        let m02 = cy * sr + mul3(sy, sp, cr);

        let m10 = sy * cr + mul3(cy, sp, sr);
        let m11 = cy * cp;
        let m12 = sy * sr - mul3(cy, sp, cr);

        let m20 = -(cp * sr);
        // `sin(pitch)` outright -- the one entry that is not already a product,
        // so it needs a `* ONE` to reach the Q60 the others land at.
        let m21 = sp * ONE;
        let m22 = cp * cr;

        Self::from_rows_unchecked([
            [entry(m00), entry(m01), entry(m02)],
            [entry(m10), entry(m11), entry(m12)],
            [entry(m20), entry(m21), entry(m22)],
        ])
    }

    /// The yaw, pitch and roll of this rotation. **ZXY intrinsic.**
    ///
    /// At the poles -- pitch at +/-a quarter turn -- yaw and roll are degenerate;
    /// roll is reported as zero and the whole turn attributed to yaw.
    #[must_use]
    #[inline]
    pub const fn to_yaw_pitch_roll(self) -> (Angle32, Pitch32, Angle32) {
        let m = self.to_rows();
        let m01 = m[0][1].to_bits() as i64;
        let m11 = m[1][1].to_bits() as i64;
        let m20 = m[2][0].to_bits() as i64;
        let m21 = m[2][1].to_bits() as i64;
        let m22 = m[2][2].to_bits() as i64;

        // `m21` is `sin(pitch)` outright, which is what the ZXY ordering buys.
        let pitch = Pitch32::asin(signed_from_q30(clamp_q30(m21) as i32));

        // Near the poles `cos(pitch)` vanishes and only a combination of yaw and
        // roll is determined; give the whole turn to yaw and report zero roll.
        //
        // Which combination it is depends on which pole: at `+90 deg` the free
        // parameter is `yaw + roll` and the top row reads `(cos, 0, sin)` of
        // it; at `-90 deg` it is `yaw - roll` and the sine comes back negated.
        if m21.abs() >= ONE - POLE_MARGIN {
            let m00 = m[0][0].to_bits() as i64;
            let m02 = m[0][2].to_bits() as i64;
            let yaw = if m21 > 0 {
                Angle32::atan2(m02, m00)
            } else {
                Angle32::atan2(-m02, m00)
            };
            return (yaw, pitch, Angle32::ZERO);
        }

        let yaw = Angle32::atan2(-m01, m11);
        let roll = Angle32::atan2(-m20, m22);
        (yaw, pitch, roll)
    }

    /// The rotation looking along `forward` with `up` overhead.
    ///
    /// `right = forward x up`, then `up = right x forward` -- so the returned
    /// basis has `forward` as column 1 and a genuinely orthonormal frame even
    /// when the supplied `up` was not perpendicular.
    ///
    /// Returns `None` when `forward` and `up` are parallel **or when either is
    /// zero-length**: a zero vector has no direction to normalize, and the
    /// parallel test alone would not catch it.
    #[must_use]
    #[inline]
    pub const fn look_to(forward: Direction, up: Direction) -> Option<Self> {
        let Some(f) = forward.normalize() else {
            return None;
        };
        // Normalized for the zero case, which the parallel test below does not
        // catch on its own.
        let Some(u0) = up.normalize() else {
            return None;
        };
        // right = forward x up, which is `X x Y = Z` read backwards.
        //
        // `cross_normalized` rather than `Direction::cross(..).normalize()`:
        // see that function for why the intermediate `Direction` cannot carry
        // this cross product.
        let Some(r) = cross_normalized(f, u0) else {
            // Parallel: the cross product vanishes.
            return None;
        };
        let Some(u) = cross_normalized(r, f) else {
            return None;
        };

        // Columns are the local axes in world space.
        Some(Self::from_rows_unchecked([
            [axis_entry(r.x()), axis_entry(f.x()), axis_entry(u.x())],
            [axis_entry(r.y()), axis_entry(f.y()), axis_entry(u.y())],
            [axis_entry(r.z()), axis_entry(f.z()), axis_entry(u.z())],
        ]))
    }

    /// The angle between two rotations.
    #[must_use]
    #[inline]
    pub const fn angle_to(self, other: Self) -> Angle32 {
        self.to_versor_const().angle_to(other.to_versor_const())
    }

    /// Steps toward `target` by at most `max_step`, never overshooting.
    #[must_use]
    #[inline]
    pub const fn rotate_towards(self, target: Self, max_step: Angle32) -> Self {
        self.to_versor_const()
            .rotate_towards(target.to_versor_const(), max_step)
            .to_basis()
    }

    /// Normalized linear interpolation, by way of the versor form.
    #[must_use]
    #[inline]
    pub const fn nlerp(self, to: Self, weight: corvid_fixed::Factor32) -> Self {
        self.to_versor_const()
            .nlerp(to.to_versor_const(), weight)
            .to_basis()
    }

    /// Spherical linear interpolation, by way of the versor form.
    #[must_use]
    #[inline]
    pub const fn slerp(self, to: Self, weight: corvid_fixed::Factor32) -> Self {
        self.to_versor_const()
            .slerp(to.to_versor_const(), weight)
            .to_basis()
    }
}
