//! [`Versor`]: a rotation as a unit quaternion of [`I2F30`] components.

use corvid_fixed::{Angle32, Factor32, I2F30};
use corvid_vector::{Direction, FinePoint, GlobalPoint};

use crate::basis::{Basis, clamp_q30, entry, round_shift_i64, signed_from_q30};
use crate::normalize::{normalize4, normalize4_fast};

/// `1.0` at the Q30 scale the components use.
const ONE: i64 = 1 << 30;

/// How far from unit-norm [`Versor::from_xyzw`] will still accept, in Q30 last
/// bits.
///
/// The same figure as `Basis`'s `ORTHONORMAL_TOLERANCE` and for the same
/// reason — room for a rotation that arrived over a wire as `f32` — but **not
/// the same window**: a basis entry is quadratic in these components, so the
/// two gates disagree on inputs from about half the tolerance up, and a versor
/// this accepts can produce a basis `Basis::from_rows` rejects.
const UNIT_TOLERANCE: i64 = 1 << 14;

/// How close two versors must be, in Q30 last bits of their dot product,
/// before [`Versor::slerp`] hands over to [`Versor::nlerp`].
///
/// `1 << 12` is a dot product of `1 - 3.8e-6`, which is `0.316°` apart. Below
/// that the `sin(θ)` the slerp weights divide by has lost most of its
/// significant bits, and the two interpolations agree to well under a last bit
/// anyway.
const SLERP_FALLBACK: i64 = 1 << 12;

/// A rotation as a unit quaternion of four [`I2F30`] components: 16 bytes.
///
/// Composing is 16 multiplies against a [`Basis`]'s 27, at 44% of the size —
/// measured at 17.6 ns against the matrix's 35.6 ns. Rotating a *point* goes
/// through the matrix form and so costs strictly more than using a [`Basis`]
/// directly: 38.5 ns against 12.6 ns. Compose as a versor, rotate as a basis.
/// (`examples/rotation_bench.rs`; the figures move with the host, the ordering
/// does not.)
///
/// Repeated composition needs [`renormalize`](Self::renormalize), which the
/// matrix does not.
/// Anything long-lived should round-trip through [`Rotation`](crate::Rotation)
/// or [`FineRotation`](crate::FineRotation): re-encoding lands on the same bits
/// every time, so a packed rotation cannot drift however often it is decoded
/// and packed again.
///
/// Components are stored in `x`, `y`, `z`, `w` order.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct Versor {
    components: [I2F30; 4],
}

impl Default for Versor {
    #[inline]
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Versor {
    /// The rotation that changes nothing: `(0, 0, 0, 1)`.
    pub const IDENTITY: Self = Self {
        components: [I2F30::ZERO, I2F30::ZERO, I2F30::ZERO, I2F30::ONE],
    };

    /// Builds a versor from four components, or `None` if they are not unit.
    ///
    /// Unit-checked for the same reason [`Basis::from_rows`] is orthonormality-
    /// checked: a non-unit versor produces a non-orthonormal `Basis`, which
    /// breaks the `i64` bound the hot path relies on.
    #[must_use]
    #[inline]
    pub const fn from_xyzw(x: I2F30, y: I2F30, z: I2F30, w: I2F30) -> Option<Self> {
        let candidate = Self {
            components: [x, y, z, w],
        };
        if (candidate.norm_squared() - ONE).abs() <= UNIT_TOLERANCE {
            Some(candidate)
        } else {
            None
        }
    }

    /// Builds a versor from components already known to be unit.
    #[inline]
    pub(crate) const fn from_xyzw_unchecked(components: [i32; 4]) -> Self {
        Self {
            components: [
                I2F30::from_bits(components[0]),
                I2F30::from_bits(components[1]),
                I2F30::from_bits(components[2]),
                I2F30::from_bits(components[3]),
            ],
        }
    }

    /// The four components, in `x`, `y`, `z`, `w` order.
    #[must_use]
    #[inline]
    pub const fn to_xyzw(self) -> [I2F30; 4] {
        self.components
    }

    /// The raw component bit patterns, in `x`, `y`, `z`, `w` order.
    #[inline]
    pub(crate) const fn bits(self) -> [i64; 4] {
        [
            self.components[0].to_bits() as i64,
            self.components[1].to_bits() as i64,
            self.components[2].to_bits() as i64,
            self.components[3].to_bits() as i64,
        ]
    }

    /// The squared norm, at Q30.
    ///
    /// Accumulated at `i128`. This runs *before*
    /// [`from_xyzw`](Self::from_xyzw)'s unit test has passed, so it sees the
    /// very components that test exists to reject: four squares of
    /// `I2F30::MAX` reach `1.85e19`, past `i64::MAX`, and any four components
    /// of `1.5` — a legal `I2F30` — already overflow. Clamping back into `i64`
    /// loses nothing near unit norm and keeps a wild input outside the
    /// tolerance window.
    #[inline]
    const fn norm_squared(self) -> i64 {
        let q = self.bits();
        let sum = ((q[0] as i128) * (q[0] as i128)
            + (q[1] as i128) * (q[1] as i128)
            + (q[2] as i128) * (q[2] as i128)
            + (q[3] as i128) * (q[3] as i128))
            >> 30;
        if sum > i64::MAX as i128 {
            i64::MAX
        } else {
            sum as i64
        }
    }

    /// Rescales back onto the unit sphere.
    ///
    /// Repeated composition accumulates a last bit at a time; this is the
    /// answer when a versor is being composed in a loop rather than
    /// round-tripped through a packed form.
    #[must_use]
    #[inline]
    pub const fn renormalize(self) -> Self {
        Self::from_xyzw_unchecked(normalize4(self.bits()))
    }

    /// Rescales back onto the unit sphere, approximately.
    ///
    /// The same routine as [`renormalize`](Self::renormalize) over
    /// [`rsqrt_fast`](I2F30::rsqrt_fast) rather than [`rsqrt`](I2F30::rsqrt),
    /// which is about 3.7x the throughput of that step.
    ///
    /// # It corrects with a deadband, not to a last bit
    ///
    /// A versor is already near unit, so the sum of squares this reduces always
    /// lands within a step of `1.0` — where the approximation happens to be
    /// exact. The consequence is not the error you would expect from `3.2e-5`;
    /// it is a **deadband**. The scale factor is quantized at `2^-15`, so any
    /// drift finer than that is invisible and passes through untouched, and the
    /// versor settles at the edge of the deadband rather than on the sphere.
    ///
    /// A million composes, each followed by a renormalize, measured:
    ///
    /// | | worst `|‖q‖² − 1|` |
    /// |---|---|
    /// | no renormalize | `8.3e-4` |
    /// | `renormalize_fast` | `1.5e-5` |
    /// | [`renormalize`](Self::renormalize) | `3.7e-9` |
    ///
    /// So it does bound the drift — it does not diverge — but it bounds it four
    /// orders of magnitude looser, and **at the edge of what
    /// [`from_xyzw`](Self::from_xyzw) accepts**. In that same loop the first
    /// output to fail `from_xyzw`'s unit test appeared after 18,619 composes.
    /// A versor that has to survive round-tripping through `from_xyzw`, or that
    /// feeds [`Basis::from_rows`](crate::Basis::from_rows), wants the exact
    /// tier.
    ///
    /// Reach for this one where the result is consumed and discarded: a versor
    /// rebuilt from `f32` interop, or one about to be packed into a form
    /// coarser than `2^-15` anyway.
    #[must_use]
    #[inline]
    pub const fn renormalize_fast(self) -> Self {
        Self::from_xyzw_unchecked(normalize4_fast(self.bits()))
    }

    /// The conjugate: the same rotation about the opposite axis.
    ///
    /// For a unit versor this is also the [`inverse`](Self::inverse).
    #[must_use]
    #[inline]
    pub const fn conjugate(self) -> Self {
        let q = self.components;
        // `saturating_neg` rather than `-bits`: a component of `I2F30::MIN` is
        // a representable value — and reachable through `bytemuck` — whose
        // plain negation overflows.
        Self {
            components: [
                q[0].saturating_neg(),
                q[1].saturating_neg(),
                q[2].saturating_neg(),
                q[3],
            ],
        }
    }

    /// The inverse rotation. For a unit versor, the conjugate.
    #[must_use]
    #[inline]
    pub const fn inverse(self) -> Self {
        self.conjugate()
    }

    /// The negated versor, which denotes the *same* rotation.
    ///
    /// The double cover: `q` and `−q` name one rotation, which is why the
    /// packed forms canonicalize a sign.
    #[must_use]
    #[inline]
    pub const fn negate(self) -> Self {
        let q = self.components;
        Self {
            components: [
                q[0].saturating_neg(),
                q[1].saturating_neg(),
                q[2].saturating_neg(),
                q[3].saturating_neg(),
            ],
        }
    }

    /// Composes two rotations, applying `rhs` **first**, then `self`.
    ///
    /// Sixteen multiplies and no `rsqrt`, which is what makes this the cheapest
    /// composition in the crate — a [`Basis`]'s costs twenty-seven multiplies.
    ///
    /// **Does not renormalize.** Each composition rounds at the last bit of
    /// [`I2F30`], so a long chain drifts slowly off the unit sphere; call
    /// [`renormalize`](Self::renormalize) when that matters, or round-trip
    /// through a packed form, whose canonical encoding cannot drift at all.
    /// Folding an `rsqrt` in here would cost an order of magnitude and hand the
    /// win straight back to the matrix.
    #[must_use]
    #[inline]
    pub const fn compose(self, rhs: Self) -> Self {
        let a = self.bits();
        let b = rhs.bits();
        // Hamilton product, at Q60 before the shift back to Q30.
        let x = a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1];
        let y = a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0];
        let z = a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3];
        let w = a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2];
        Self {
            components: [entry(x), entry(y), entry(z), entry(w)],
        }
    }

    /// The dot product of two versors, as a value in `[-1, 1]`.
    ///
    /// `1` means the same rotation, `-1` the same rotation reached the other
    /// way round the double cover.
    #[must_use]
    #[inline]
    pub const fn dot(self, other: Self) -> I2F30 {
        let a = self.bits();
        let b = other.bits();
        let sum = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
        entry(sum)
    }

    /// Returns `true` if every component is within `tolerance` of `other`'s.
    ///
    /// Compares components, not rotations: `q` and `−q` are the same rotation
    /// and will *not* compare equal here. Use
    /// [`angle_to`](crate::Versor::angle_to) to compare rotations.
    #[must_use]
    #[inline]
    pub const fn abs_diff_eq(self, other: Self, tolerance: I2F30) -> bool {
        let limit = tolerance.to_bits() as i64;
        let (a, b) = (self.bits(), other.bits());
        let mut i = 0;
        while i < 4 {
            if (a[i] - b[i]).abs() > limit {
                return false;
            }
            i += 1;
        }
        true
    }

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
    /// Every entry lands in `[-1, 1]`, comfortably inside [`I2F30`], and each
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
    /// Branches on the largest of the four candidate denominators — the same
    /// chart idea the 32-bit codec uses — so the division is never by a value
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

        // The four candidates are `4w²`, `4x²`, `4y²`, `4z²` minus one, at Q30.
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
    /// costs an `acos` and two `sin`s — the two slowest functions in
    /// `corvid_fixed`.
    ///
    /// Interpolates along the short way round: if the two versors are more than
    /// a half turn apart on the double cover, `to` is negated first.
    ///
    /// # Exact at both ends
    ///
    /// At [`Factor32::ZERO`] this returns `self` and at [`Factor32::ONE`] it
    /// returns `to`, bit for bit and including the sign the caller passed `to`
    /// in with. Both are short-circuits rather than what the arithmetic
    /// happens to produce: the renormalize this ends on moves a component of an
    /// already-unit versor about half the time, so without them an endpoint
    /// came back naming the same rotation as the one that went in and not
    /// carrying the same bits. A capture compares bytes, so `nearly` is a
    /// different property from `exactly` here, and this is the cheaper of the
    /// two to hold.
    #[must_use]
    #[inline]
    pub const fn nlerp(self, to: Self, weight: Factor32) -> Self {
        // Before the negation below, so that the versor handed back at `ONE` is
        // the one the caller passed rather than its double-cover twin — the
        // same choice [`rotate_towards`](Self::rotate_towards) makes, and for
        // the same reason: `Versor`'s equality is on the bits.
        if weight.to_bits() == Factor32::ONE.to_bits() {
            return to;
        }
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
        // A zero weight is `self` whichever side of the double cover `to` came
        // in on, so this guard serves the public entry point and the
        // zero-length step `rotate_towards` routes through here alike.
        if weight.to_bits() == 0 {
            return self;
        }
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
    ///
    /// Exact at both ends, the way [`nlerp`](Self::nlerp) is and for the same
    /// reason.
    #[must_use]
    #[inline]
    pub const fn slerp(self, to: Self, weight: Factor32) -> Self {
        // As in `nlerp`: before the negation, so `to` comes back as it was
        // passed.
        if weight.to_bits() == Factor32::ONE.to_bits() {
            return to;
        }
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
    /// Both public entry points already hold those two values, and `acos` is the
    /// slowest function the crate has: a `slerp` that recomputed the dot product
    /// would cost [`rotate_towards`](Self::rotate_towards) a second `acos`, which
    /// is about a third of what that call costs in total.
    #[inline]
    pub(crate) const fn slerp_canonical(self, to: Self, weight: Factor32, cosine: i64) -> Self {
        // As in `nlerp_canonical`, and this is the guard that makes a
        // `rotate_towards` of no angle at all leave the rotation alone: it
        // arrives here with a zero fraction, and the arithmetic below would
        // renormalize `self` into a neighbouring bit pattern.
        if weight.to_bits() == 0 {
            return self;
        }
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

        // The two weights, `sin((1-t)θ)/sin(θ)` and `sin(tθ)/sin(θ)`.
        let t = weight.to_bits() as u64;
        let full = Factor32::MAX.to_bits() as u64;
        let scaled = ((theta.to_bits() as u64) * t / full) as u32;
        let from_weight = Angle32::from_bits(theta.to_bits() - scaled).sin().to_bits() as i64;
        let to_weight = Angle32::from_bits(scaled).sin().to_bits() as i64;

        let (a, b) = (self.bits(), to.bits());
        let mut mixed = [0i64; 4];
        let mut i = 0;
        while i < 4 {
            // Scale-free: `normalize4` divides the common `1/sin(θ)` out.
            mixed[i] = round_shift_i64(a[i] * from_weight + b[i] * to_weight, 31);
            i += 1;
        }
        Self::from_xyzw_unchecked(normalize4(mixed))
    }
}

impl From<Versor> for Basis {
    #[inline]
    fn from(q: Versor) -> Self {
        q.to_basis()
    }
}

impl From<Basis> for Versor {
    #[inline]
    fn from(m: Basis) -> Self {
        Self::from_basis(m)
    }
}

impl core::fmt::Debug for Versor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let q = self.components;
        write!(
            f,
            "Versor({}, {}, {}, {})",
            q[0].to_f64(),
            q[1].to_f64(),
            q[2].to_f64(),
            q[3].to_f64()
        )
    }
}
