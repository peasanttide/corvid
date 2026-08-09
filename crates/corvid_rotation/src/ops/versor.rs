//! The operation family on [`Versor`], which is where a rotation is built and
//! composed.

use corvid_fixed::{Angle32, Pitch32, Signed32};
use corvid_vector::Direction;

use super::{ONE, angle_from_cosine, perpendicular_to, q30_from_signed};
use crate::basis::{Basis, clamp_q30, round_shift_i64, signed_from_q30};
use crate::normalize::normalize4;
use crate::versor::Versor;

impl Versor {
    /// The rotation of `angle` about `axis`.
    ///
    /// `axis` should be a unit direction; the result is renormalized either
    /// way, so a slightly-off axis costs accuracy rather than correctness.
    #[must_use]
    #[inline]
    pub const fn from_axis_angle(axis: Direction, angle: Angle32) -> Self {
        // `Angle32` spans a whole turn over `u32`, so halving the angle is an
        // unsigned shift and nothing else.
        let half = Angle32::from_bits(angle.to_bits() >> 1);
        let (sin, cos) = half.sin_cos();
        let s = q30_from_signed(sin);
        let [x, y, z] = axis.to_array();

        Self::from_xyzw_unchecked(normalize4([
            round_shift_i64(q30_from_signed(x) * s, 30),
            round_shift_i64(q30_from_signed(y) * s, 30),
            round_shift_i64(q30_from_signed(z) * s, 30),
            q30_from_signed(cos),
        ]))
    }

    /// The axis and angle this rotation turns through.
    ///
    /// The angle is in `0 ..= half a turn` and the axis points so that the
    /// rotation is right-handed about it. For the identity the axis is
    /// arbitrary; **+Z** is returned.
    #[must_use]
    #[inline]
    pub const fn to_axis_angle(self) -> (Direction, Angle32) {
        // Canonicalize onto the hemisphere with a non-negative scalar part, so
        // the angle comes back in the first half turn.
        let q = if self.to_xyzw()[3].is_negative() {
            self.negate()
        } else {
            self
        };
        let c = q.bits();

        let half = Angle32::acos(signed_from_q30(clamp_q30(c[3]) as i32));
        let angle = Angle32::from_bits(half.to_bits().wrapping_mul(2));

        match Direction::new(
            signed_from_q30(clamp_q30(c[0]) as i32),
            signed_from_q30(clamp_q30(c[1]) as i32),
            signed_from_q30(clamp_q30(c[2]) as i32),
        )
        .normalize()
        {
            Some(axis) => (axis, angle),
            // The identity, whose axis is arbitrary.
            None => (
                Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MAX),
                angle,
            ),
        }
    }

    /// The rotation taking `from` onto `to`, along the shortest arc.
    ///
    /// Total. Identical inputs give the identity; antipodal inputs give a half
    /// turn about some perpendicular axis, which is the honest answer when the
    /// shortest arc is not unique.
    #[must_use]
    #[inline]
    pub const fn from_rotation_arc(from: Direction, to: Direction) -> Self {
        let f = [
            q30_from_signed(from.x()),
            q30_from_signed(from.y()),
            q30_from_signed(from.z()),
        ];
        let t = [
            q30_from_signed(to.x()),
            q30_from_signed(to.y()),
            q30_from_signed(to.z()),
        ];

        let dot = round_shift_i64(f[0] * t[0] + f[1] * t[1] + f[2] * t[2], 30);
        let cross = [
            round_shift_i64(f[1] * t[2] - f[2] * t[1], 30),
            round_shift_i64(f[2] * t[0] - f[0] * t[2], 30),
            round_shift_i64(f[0] * t[1] - f[1] * t[0], 30),
        ];

        // Antipodal: neither the axis nor the angle is determined any more, so
        // any perpendicular axis is as good as any other. Take the one furthest
        // from `from`.
        //
        // The test is on `dot` alone. Also requiring the cross product to be
        // *exactly* zero -- as this once did -- narrowed the branch to exactly
        // opposite inputs and nothing else: at the edge of this window the two
        // are `0.04 deg` from opposite, where the cross is still about `2^19` at
        // Q30. Everything between there and exact opposition fell through to
        // the formula below, where `1 + dot` has underflowed to a handful of
        // last bits and the cross carries as few, and came back a rotation
        // missing `to` by degrees -- 2.8 deg at `0.006 deg` of separation, over 100 deg
        // below that.
        if dot <= -ONE + (1 << 8) {
            let axis = perpendicular_to(f);
            return Self::from_xyzw_unchecked(normalize4([axis[0], axis[1], axis[2], 0]));
        }

        // `q = (cross, 1 + dot)`, the half-way quaternion, normalized.
        Self::from_xyzw_unchecked(normalize4([cross[0], cross[1], cross[2], ONE + dot]))
    }

    /// Builds a rotation from yaw, pitch and roll. **ZXY intrinsic.**
    #[must_use]
    #[inline]
    pub const fn from_yaw_pitch_roll(yaw: Angle32, pitch: Pitch32, roll: Angle32) -> Self {
        Basis::from_yaw_pitch_roll(yaw, pitch, roll).to_versor_const()
    }

    /// The yaw, pitch and roll of this rotation. **ZXY intrinsic.**
    #[must_use]
    #[inline]
    pub const fn to_yaw_pitch_roll(self) -> (Angle32, Pitch32, Angle32) {
        self.to_basis().to_yaw_pitch_roll()
    }

    /// The rotation looking along `forward` with `up` overhead.
    ///
    /// Returns `None` when `forward` and `up` are parallel **or when either is
    /// zero-length** -- a zero vector has no direction to normalize, and the
    /// parallel test alone would not catch it.
    #[must_use]
    #[inline]
    pub const fn look_to(forward: Direction, up: Direction) -> Option<Self> {
        match Basis::look_to(forward, up) {
            Some(m) => Some(m.to_versor_const()),
            None => None,
        }
    }

    /// The angle between two rotations, in `0 ..= half a turn`.
    ///
    /// Uses `|dot|`, so the double cover does not double the answer.
    ///
    /// # Precision near zero
    ///
    /// This is the `acos` form, and `acos` is ill-conditioned at `1`: near
    /// zero the answer goes as `sqrt(2 * epsilon)` in the dot product's error, so a
    /// last
    /// bit of `I2F30` -- `9.3e-10` -- becomes about **0.0025 deg** of reported
    /// angle. Two rotations this function calls 0.002 deg apart may be bit-
    /// identical.
    ///
    /// That is fine for what the operation is for -- steering, thresholds,
    /// [`rotate_towards`](Self::rotate_towards) -- but it makes `angle_to` the
    /// wrong tool for *measuring* a codec, which is why the crate's own error
    /// statistics use the chord form `4*asin(chord/2)` in `f64` instead. To
    /// compare two rotations for near-equality, reach for
    /// [`abs_diff_eq`](Self::abs_diff_eq) on the components.
    #[must_use]
    #[inline]
    pub const fn angle_to(self, other: Self) -> Angle32 {
        let cosine = self.dot(other).to_bits() as i64;
        angle_from_cosine(clamp_q30(cosine.abs()))
    }

    /// Steps toward `target` by at most `max_step`.
    ///
    /// Never overshoots: when the remaining angle is under `max_step` the
    /// target is returned exactly. Pays for an `acos` to know the true angle,
    /// which is why [`nlerp`](Self::nlerp) exists for the per-frame case.
    #[must_use]
    #[inline]
    pub const fn rotate_towards(self, target: Self, max_step: Angle32) -> Self {
        // One dot and one `acos` for the whole call. Going through `angle_to`
        // and then `slerp` computed both twice -- and `acos` is the slowest
        // function in `corvid_fixed`, so on a per-entity-per-frame steering
        // call that second one was about a third of the cost.
        let signed = self.dot(target).to_bits() as i64;
        let cosine = clamp_q30(signed.abs());
        let remaining = angle_from_cosine(cosine);
        if remaining.to_bits() <= max_step.to_bits() || remaining.to_bits() == 0 {
            // The *original* `target`, not the double-cover twin below: the
            // documented guarantee is that the target comes back exactly, and
            // `Versor`'s equality is on the bits.
            return target;
        }
        // The fraction of the way to travel, as a `Factor32`.
        let fraction = ((max_step.to_bits() as u64)
            * (corvid_fixed::Factor32::MAX.to_bits() as u64)
            / (remaining.to_bits() as u64)) as u32;
        let short = if signed < 0 { target.negate() } else { target };
        self.slerp_canonical(short, corvid_fixed::Factor32::from_bits(fraction), cosine)
    }
}
