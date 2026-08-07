//! [`Rotation`]: the 32-bit tier, a Gibbs vector in 2+10+10+10 bits.

use crate::basis::Basis;
use crate::normalize::normalize4;
use crate::versor::Versor;

/// Bits per Gibbs component.
const FIELD_BITS: u32 = 10;

/// Mask selecting one component field.
const FIELD_MASK: u32 = (1 << FIELD_BITS) - 1;

/// The largest magnitude a field can hold, so `±1.0` is exactly representable.
///
/// The crate's signed-normalized convention: a field denotes `v / MAX`, with
/// `v` in `-511 ..= 511`, stored offset by 512.
const FIELD_MAX: i64 = (1 << (FIELD_BITS - 1)) - 1;

/// The offset that makes a signed field an unsigned bit pattern.
const FIELD_BIAS: i32 = 1 << (FIELD_BITS - 1);

/// A rotation packed into 32 bits: **0.0784° mean, 0.1832° max** over uniform
/// SO(3).
///
/// A 2-bit chart index selects the largest-magnitude quaternion component; the
/// other three are divided by it, giving the Gibbs vector `t = tan(θ/2)·axis`,
/// which lies in exactly the cube `[-1, 1]³`. Three 10-bit fields store `t`.
///
/// This clears the 1/5° budget and is the cheapest decode in the family.
/// Alternatives measured and rejected for this tier:
///
/// | codec | mean | max | decode work beyond the shared normalize |
/// |---|---|---|---|
/// | **gibbs linear 2+10+10+10** | 0.0784° | **0.1832°** | none |
/// | gibbs bcc linear 2+1+29 | 0.0766° | 0.1528° | 2 int div/mod by N=812 |
/// | smallest-three (baseline) | 0.0844° | 0.2423° | — misses the budget |
///
/// Every rejected codec performs the same normalize and then strictly more
/// work, so the ranking holds regardless of what the integer costs turn out to
/// be. The figures are `examples/rotation_quality.rs` over 200,000 uniform
/// samples against an `f64` reference, which is the same run the crate README
/// quotes — stated once there rather than re-transcribed per type.
///
/// # Layout
///
/// Low to high: `t[a]` in bits 0–9, `t[b]` in bits 10–19, `t[c]` in bits 20–29,
/// chart index in bits 30–31, where `a < b < c` are the three component indices
/// other than the chart index, ascending over `[x, y, z, w]`.
///
/// Every `u32` is a valid `Rotation` and decodes to a unit quaternion.
///
/// # Equality is on the bits, not on the rotation
///
/// Unlike [`FineRotation`](crate::FineRotation), whose `Eq` and [`Hash`] route
/// through its own cheap canonicalization, this type compares and hashes its
/// raw `u32`. The encoding is **not** injective: 0.58% of arbitrary `u32`
/// patterns decode to a quaternion that re-encodes to different bits — the
/// one-past-the-end field pattern folds onto `-511`, and a Gibbs vector sitting
/// on a chart boundary can name either of two charts. Two such patterns are the
/// same rotation and still compare unequal.
///
/// Encoding narrows that; it does not close it. Of the patterns the encoder
/// itself produced, 0.065% are still non-canonical, the residue being the chart
/// ties: where two quaternion components are equal in magnitude either can serve
/// as the chart, and a re-encode picks the lower-indexed one and requantizes
/// there. So the case to plan for is mostly a value that arrived as raw bits —
/// over a wire, from `bytemuck`, from `arbitrary` — but not only that.
/// [`canonicalize`](Self::canonicalize) is what settles either, and it is
/// idempotent, so one pass is enough. It is not free: unlike `FineRotation`'s,
/// it costs a decode and a re-encode, which is why it is not folded into `Eq`.
///
/// Both figures are measured in `tests/rotation32.rs`, over a million arbitrary
/// patterns and a hundred thousand encoded ones.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "arbitrary", derive(::arbitrary::Arbitrary))]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct Rotation(u32);

impl Default for Rotation {
    #[inline]
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Rotation {
    /// The rotation that changes nothing: chart `w`, Gibbs vector zero.
    pub const IDENTITY: Self = Self(3 << (3 * FIELD_BITS) | encode_zero_fields());

    /// Wraps a raw bit pattern.
    ///
    /// Every pattern is valid and decodes to a unit quaternion, so this is the
    /// exact inverse of [`to_bits`](Self::to_bits) and is what `bytemuck` and
    /// `serde` round-trip through.
    #[must_use]
    #[inline]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// The raw bit pattern.
    #[must_use]
    #[inline]
    pub const fn to_bits(self) -> u32 {
        self.0
    }

    /// Packs a unit quaternion.
    #[must_use]
    #[inline]
    pub const fn from_versor(q: Versor) -> Self {
        let mut c = q.bits();

        // Chart: the largest-magnitude component. The double cover lets us
        // force it positive, which is what makes the Gibbs division safe.
        let mut chart = 0;
        let mut i = 1;
        while i < 4 {
            if c[i].unsigned_abs() > c[chart].unsigned_abs() {
                chart = i;
            }
            i += 1;
        }
        if c[chart] < 0 {
            c = [-c[0], -c[1], -c[2], -c[3]];
        }

        // The pivot is now the largest positive component, so each ratio lands
        // in `[-1, 1]` and the division cannot blow up.
        let pivot = c[chart];
        let mut bits = (chart as u32) << (3 * FIELD_BITS);
        let mut slot = 0;
        let mut index = 0;
        while index < 4 {
            if index != chart {
                let field = if pivot == 0 {
                    // Only reachable from an all-zero versor, which no public
                    // constructor produces; a zero Gibbs vector is the honest
                    // answer for it.
                    0
                } else {
                    quantize(c[index], pivot)
                };
                bits |= ((field + FIELD_BIAS) as u32 & FIELD_MASK) << (slot * FIELD_BITS);
                slot += 1;
            }
            index += 1;
        }
        Self(bits)
    }

    /// Unpacks into a unit quaternion.
    ///
    /// Integer-only: swizzle the three fields back around the chart index, pin
    /// the chart slot to the field scale, and hand all four to the shared
    /// normalize. That normalize is *not* free — it is the dominant cost of the
    /// decode, which is why `corvid_fixed` grew an `rsqrt` rather than
    /// composing `sqrt` and `recip`.
    #[must_use]
    #[inline]
    pub const fn to_versor(self) -> Versor {
        let chart = (self.0 >> (3 * FIELD_BITS)) as usize & 3;

        // The pivot takes the field scale, so the four numbers share one scale
        // and `normalize4` — which cares only about ratios — needs no division
        // by the field maximum.
        let mut raw = [0i64; 4];
        raw[chart] = FIELD_MAX;
        let mut slot = 0;
        let mut index = 0;
        while index < 4 {
            if index != chart {
                let field = ((self.0 >> (slot * FIELD_BITS)) & FIELD_MASK) as i32;
                // The biased range reaches `-512`, one step past what the
                // encoder emits — the signed-normalized convention keeps `±1`
                // symmetric, so `MIN` is `-MAX`. Folding that one pattern onto
                // `-511` keeps the pivot the largest component for *every*
                // `u32`, so every pattern decodes to a genuine rotation. It
                // also means two patterns can decode alike; see the type's own
                // note on equality.
                let value = field - FIELD_BIAS;
                raw[index] = if value < -(FIELD_MAX as i32) {
                    -FIELD_MAX
                } else {
                    value as i64
                };
                slot += 1;
            }
            index += 1;
        }
        Versor::from_xyzw_unchecked(normalize4(raw))
    }

    /// The canonical bit pattern for this rotation.
    ///
    /// A decode and a re-encode — the chart choice and the field quantization
    /// are only recoverable that way, so unlike
    /// [`FineRotation::canonicalize`](crate::FineRotation::canonicalize) this
    /// is not a handful of compares. Idempotent, so it is safe to use as a map
    /// key or an equality test on patterns that arrived as raw bits.
    #[must_use]
    #[inline]
    pub const fn canonicalize(self) -> Self {
        Self::from_versor(self.to_versor())
    }

    /// Returns `true` if this is already the canonical encoding.
    #[must_use]
    #[inline]
    pub const fn is_canonical(self) -> bool {
        self.0 == self.canonicalize().0
    }

    /// Packs a rotation matrix.
    #[must_use]
    #[inline]
    pub const fn from_basis(m: Basis) -> Self {
        Self::from_versor(Versor::from_basis(m))
    }

    /// Unpacks into a rotation matrix.
    #[must_use]
    #[inline]
    pub const fn to_basis(self) -> Basis {
        self.to_versor().to_basis()
    }
}

/// The three zero fields of the identity, already biased.
const fn encode_zero_fields() -> u32 {
    let field = FIELD_BIAS as u32;
    field | (field << FIELD_BITS) | (field << (2 * FIELD_BITS))
}

/// `round(value / pivot * FIELD_MAX)`, clamped into a field.
///
/// `pivot` is the largest magnitude among the four components and is positive,
/// so the ratio is in `[-1, 1]` and the clamp only ever catches a rounding tie
/// at the very edge.
#[inline]
const fn quantize(value: i64, pivot: i64) -> i32 {
    let scaled = value * FIELD_MAX;
    let rounded = if scaled >= 0 {
        (2 * scaled + pivot) / (2 * pivot)
    } else {
        -((-2 * scaled + pivot) / (2 * pivot))
    };
    if rounded > FIELD_MAX {
        FIELD_MAX as i32
    } else if rounded < -FIELD_MAX {
        -(FIELD_MAX as i32)
    } else {
        rounded as i32
    }
}

impl From<Versor> for Rotation {
    #[inline]
    fn from(q: Versor) -> Self {
        Self::from_versor(q)
    }
}

impl From<Rotation> for Versor {
    #[inline]
    fn from(r: Rotation) -> Self {
        r.to_versor()
    }
}

impl From<Basis> for Rotation {
    #[inline]
    fn from(m: Basis) -> Self {
        Self::from_basis(m)
    }
}

impl From<Rotation> for Basis {
    #[inline]
    fn from(r: Rotation) -> Self {
        r.to_basis()
    }
}

impl core::fmt::Debug for Rotation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Rotation({:#010x} -> {:?})", self.0, self.to_versor())
    }
}
