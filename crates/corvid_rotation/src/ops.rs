//! The rotation operation family: axis-angle, Euler, `look_to`, arcs and steps.
//!
//! Everything is written once against [`Versor`] and forwarded from [`Basis`],
//! except where the matrix form is strictly better — [`Basis::look_to`] and
//! [`Basis::from_yaw_pitch_roll`] build the matrix directly, because their
//! answer *is* a set of axes.
//!
//! # Conventions this module nails down
//!
//! Yaw rotates about **+Z**, pitch about **+X**, roll about **+Y**, and Euler
//! composition is **ZXY intrinsic**: `R = Rz(yaw) · Rx(pitch) · Ry(roll)`. Yaw
//! and roll take [`Angle32`] because they wrap; pitch takes [`Pitch32`] because
//! it clamps, which is exactly right for a head pose — looking too far up
//! leaves you looking up rather than upside down.
//!
//! `right = forward × up`, consistent with `X × Y = Z`.

use corvid_fixed::{Angle32, I2F30, Pitch32, Signed32};
use corvid_vector::Direction;

use crate::basis::{Basis, clamp_q30, entry, round_shift_i64, signed_from_q30};
use crate::normalize::{normalize4, shift_down};
use crate::versor::Versor;

/// `1.0` at the Q30 scale.
const ONE: i64 = 1 << 30;

/// How close `sin(pitch)` must come to `±1` before yaw and roll are treated as
/// degenerate, in Q30 last bits.
///
/// Derived rather than picked. Outside the branch `to_yaw_pitch_roll` reads
/// yaw off `atan2(-m01, m11)`, whose two arguments are both `cos(pitch)` times
/// something bounded by one; the quantization floor is a Q30 last bit, so the
/// bearing carries about `log2(cos(pitch) · 2^30)` bits. With
/// `|m21| = 1 − k/2^30`, `cos(pitch) ≈ √(2k)·2^-15`, so `k = 1 << 7` leaves
/// `cos(pitch) ≥ 4.9e-4` — 19 bits, an angular floor near `1e-4°`, comfortably
/// under the `0.005°` the codec itself carries.
///
/// A wider margin costs accuracy rather than buying it. At `1 << 12` the branch
/// fires from **89.84°**, where `cos(pitch)` is still `2.8e-3` and roll is fully
/// determined; discarding it and attributing the whole turn to yaw costs `0.30°`
/// of round-trip error — 60× the codec floor — in a band head tracking passes
/// through routinely.
const POLE_MARGIN: i64 = 1 << 7;

/// Converts a [`Signed32`] into a Q30 bit pattern, rounded once.
///
/// Reads the *canonical* bit pattern. `Signed32` spends `i32::MIN` and
/// `-(2^31 - 1)` on the same `-1.0` and folds the denormal on the way into its
/// own arithmetic; this does the same, so two components that compare and hash
/// equal cannot produce different rotations.
#[inline]
const fn q30_from_signed(value: Signed32) -> i64 {
    let scaled = (value.canonicalize().to_bits() as i64) << 30;
    let denominator = Signed32::MAX.to_bits() as i64;
    if scaled >= 0 {
        (2 * scaled + denominator) / (2 * denominator)
    } else {
        -((-2 * scaled + denominator) / (2 * denominator))
    }
}

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
        // *exactly* zero would narrow the branch to exactly opposite inputs and
        // nothing else: at the edge of this window the two are `0.04°` from
        // opposite, where the cross is still about `2^19` at Q30. Everything
        // between there and exact opposition would fall through to the formula
        // below, where `1 + dot` has underflowed to a handful of last bits and
        // the cross carries as few, and would come back a rotation missing `to`
        // by degrees — 2.8° at `0.006°` of separation, over 100° below that.
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
    /// zero-length** — a zero vector has no direction to normalize, and the
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
    /// zero the answer goes as `√(2ε)` in the dot product's error, so a last
    /// bit of `I2F30` — `9.3e-10` — becomes about **0.0025°** of reported
    /// angle. Two rotations this function calls 0.002° apart may be bit-
    /// identical.
    ///
    /// That is fine for what the operation is for — steering, thresholds,
    /// [`rotate_towards`](Self::rotate_towards) — but it makes `angle_to` the
    /// wrong tool for *measuring* a codec, which is why the crate's own error
    /// statistics use the chord form `4·asin(chord/2)` in `f64` instead. To
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
    /// target is returned exactly, and a `max_step` of [`Angle32::ZERO`] leaves
    /// this versor exactly where it is. Pays for an `acos` to know the true
    /// angle, which is why [`nlerp`](Self::nlerp) exists for the per-frame
    /// case.
    ///
    /// The zero step is decided before the angle is measured and not by it,
    /// because the measurement cannot decide it. See
    /// [`angle_to`](Self::angle_to): the `acos` reports a flat zero for any
    /// pair closer than about 0.0025°, so `remaining <= max_step` is *true* at
    /// a zero step for two rotations that are genuinely two rotations, and a
    /// guard that then returns the target has moved a caller who asked to
    /// stand still.
    #[must_use]
    #[inline]
    pub const fn rotate_towards(self, target: Self, max_step: Angle32) -> Self {
        if max_step.to_bits() == 0 {
            return self;
        }
        // One dot and one `acos` for the whole call. Going through `angle_to`
        // and then `slerp` computed both twice — and `acos` is the slowest
        // function in `corvid_fixed`, so on a per-entity-per-frame steering
        // call that second one was about a third of the cost.
        let signed = self.dot(target).to_bits() as i64;
        let cosine = clamp_q30(signed.abs());
        let remaining = angle_from_cosine(cosine);
        if remaining.to_bits() <= max_step.to_bits() {
            // The *original* `target`, not the double-cover twin below: the
            // documented guarantee is that the target comes back exactly, and
            // `Versor`'s equality is on the bits. A `remaining` of zero lands
            // here too, and should: `max_step` is non-zero by now, so the step
            // covers every gap the measurement can see.
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

/// Twice the `acos` of a non-negative Q30 cosine: the angle between two
/// rotations, in `0 ..= half a turn`.
#[inline]
const fn angle_from_cosine(cosine: i64) -> Angle32 {
    let half = Angle32::acos(signed_from_q30(cosine as i32));
    Angle32::from_bits(half.to_bits().wrapping_mul(2))
}

/// Some unit vector perpendicular to `v`, at Q30.
///
/// Crosses with whichever cardinal axis `v` leans on least, which is never
/// degenerate.
#[inline]
const fn perpendicular_to(v: [i64; 3]) -> [i64; 3] {
    if v[0].abs() <= v[1].abs() && v[0].abs() <= v[2].abs() {
        [0, -v[2], v[1]]
    } else if v[1].abs() <= v[2].abs() {
        [-v[2], 0, v[0]]
    } else {
        [-v[1], v[0], 0]
    }
}

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
    /// `R = Rz(yaw) · Rx(pitch) · Ry(roll)`.
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
        // `sin(pitch)` outright — the one entry that is not already a product,
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
    /// At the poles — pitch at ±a quarter turn — yaw and roll are degenerate;
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
        // Which combination it is depends on which pole: at `+90°` the free
        // parameter is `yaw + roll` and the top row reads `(cos, 0, sin)` of
        // it; at `-90°` it is `yaw − roll` and the sine comes back negated.
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
    /// `right = forward × up`, then `up = right × forward` — so the returned
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
    ///
    /// Lands on `target` exactly once the step covers the remaining angle, and
    /// on `self` exactly for a `max_step` of [`Angle32::ZERO`] — the zero step
    /// unconditionally, for the reason
    /// [`Versor::rotate_towards`](Versor::rotate_towards) gives.
    #[must_use]
    #[inline]
    pub const fn rotate_towards(self, target: Self, max_step: Angle32) -> Self {
        // First, and at this tier rather than only inside the versor: a matrix
        // does not survive the round trip below, and two matrices a rounding
        // apart share one versor, in which case the `to` test would win and
        // hand back the other one.
        if max_step.to_bits() == 0 {
            return self;
        }
        let (from, to) = (self.to_versor_const(), target.to_versor_const());
        let stepped = from.rotate_towards(to, max_step);
        // The endpoints are recognised here rather than left to the round trip,
        // because the round trip is what loses them: `Versor::rotate_towards`
        // hands an endpoint back bit for bit, and then `from_basis` and
        // `to_basis` each renormalize, so a matrix that went out and came back
        // is within a rounding of the one it started as and not on it.
        if same_versor(stepped, to) {
            target
        } else if same_versor(stepped, from) {
            self
        } else {
            stepped.to_basis()
        }
    }

    /// Normalized linear interpolation, by way of the versor form.
    ///
    /// Exact at both ends: `ZERO` is `self` and `ONE` is `to`, bit for bit.
    #[must_use]
    #[inline]
    pub const fn nlerp(self, to: Self, weight: corvid_fixed::Factor32) -> Self {
        // Checked at this tier and not only inside `Versor::nlerp`, because the
        // matrix does not survive its own conversion to versor form and back.
        match endpoint(self, to, weight) {
            Some(end) => end,
            None => self
                .to_versor_const()
                .nlerp(to.to_versor_const(), weight)
                .to_basis(),
        }
    }

    /// Spherical linear interpolation, by way of the versor form.
    ///
    /// Exact at both ends, the way [`nlerp`](Self::nlerp) is.
    #[must_use]
    #[inline]
    pub const fn slerp(self, to: Self, weight: corvid_fixed::Factor32) -> Self {
        match endpoint(self, to, weight) {
            Some(end) => end,
            None => self
                .to_versor_const()
                .slerp(to.to_versor_const(), weight)
                .to_basis(),
        }
    }
}

/// Whichever basis an interpolation at `weight` is required to reproduce
/// exactly, and [`None`] anywhere between the two ends.
#[inline]
const fn endpoint(from: Basis, to: Basis, weight: corvid_fixed::Factor32) -> Option<Basis> {
    if weight.to_bits() == 0 {
        Some(from)
    } else if weight.to_bits() == corvid_fixed::Factor32::ONE.to_bits() {
        Some(to)
    } else {
        None
    }
}

/// Whether two versors carry the same four bit patterns.
///
/// `PartialEq` says this too, and cannot be called from a `const fn`.
#[inline]
const fn same_versor(a: Versor, b: Versor) -> bool {
    let (a, b) = (a.to_xyzw(), b.to_xyzw());
    let mut i = 0;
    while i < 4 {
        if a[i].to_bits() != b[i].to_bits() {
            return false;
        }
        i += 1;
    }
    true
}

/// The product of three Q30 values, brought back to Q60.
#[inline]
const fn mul3(a: i64, b: i64, c: i64) -> i64 {
    round_shift_i64(a * b, 30) * c
}

/// A `Signed32` axis component as an `I2F30` matrix entry.
#[inline]
const fn axis_entry(value: Signed32) -> I2F30 {
    I2F30::from_bits(q30_from_signed(value) as i32)
}

/// `a × b`, normalized, without the round trip through a unit-scaled
/// [`Direction`].
///
/// [`Direction::cross`] divides its `i64` cross terms back onto `Signed32`'s
/// `±1` before returning, which keeps only the bits *above* the cross product's
/// own magnitude. For two nearly parallel directions that magnitude is tiny —
/// it goes as the sine of the angle between them — so almost nothing survives
/// the division, and the `normalize` that follows amplifies what is left of the
/// rounding rather than a direction. At `0.006°` of separation that made
/// [`Basis::look_to`] hand back a frame skewed by a third of a degree, and a
/// tenth of that separation cost ten.
///
/// Rescaling the terms by a shift instead of dividing them keeps every bit the
/// `i64` products carried; [`Direction::normalize`] cares only about ratios, so
/// the shift costs nothing at all.
///
/// `None` when the two are parallel — including when either is zero — which is
/// the only case with no answer.
#[inline]
const fn cross_normalized(a: Direction, b: Direction) -> Option<Direction> {
    // Canonical bits: `Signed32` spends `i32::MIN` and `-(2^31 - 1)` on the
    // same `-1.0`, and reading the raw pattern would make two components that
    // compare equal cross to different axes.
    let ax = a.x().canonicalize().to_bits() as i64;
    let ay = a.y().canonicalize().to_bits() as i64;
    let az = a.z().canonicalize().to_bits() as i64;
    let bx = b.x().canonicalize().to_bits() as i64;
    let by = b.y().canonicalize().to_bits() as i64;
    let bz = b.z().canonicalize().to_bits() as i64;

    // Each product is at most `(2^31 - 1)^2` and a difference of two of them
    // reaches `2 * (2^31 - 1)^2`, which is still under `i64::MAX`.
    let c = [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx];

    let mut largest = c[0].unsigned_abs();
    let mut i = 1;
    while i < 3 {
        if c[i].unsigned_abs() > largest {
            largest = c[i].unsigned_abs();
        }
        i += 1;
    }
    if largest == 0 {
        return None;
    }

    // Bring the largest term into `[2^30, 2^31)` so the whole triple survives
    // the narrowing to `i32` with every bit it had.
    let bit_length = corvid_bits::bit_length_u64(largest);
    let scaled = if bit_length > 31 {
        let down = bit_length - 31;
        [
            shift_down(c[0], down),
            shift_down(c[1], down),
            shift_down(c[2], down),
        ]
    } else {
        let up = 31 - bit_length;
        [c[0] << up, c[1] << up, c[2] << up]
    };

    Direction::new(
        Signed32::from_bits(scaled[0] as i32),
        Signed32::from_bits(scaled[1] as i32),
        Signed32::from_bits(scaled[2] as i32),
    )
    .normalize()
}
