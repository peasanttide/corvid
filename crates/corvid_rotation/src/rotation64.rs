//! [`FineRotation`]: the 64-bit tier, four `Signed16` quaternion components.

use corvid_fixed::Signed16;

use crate::basis::{Basis, round_shift_i64};
use crate::normalize::normalize4;
use crate::rotation32::Rotation;
use crate::versor::Versor;

/// The largest magnitude a component holds. `Signed16`'s SNORM scale.
const COMPONENT_MAX: i64 = i16::MAX as i64;

/// The low 16 bits of `raw` as a component value, with `i16::MIN` folded onto
/// `-COMPONENT_MAX`.
#[inline]
const fn fold(raw: u64) -> i64 {
    let value = ((raw & 0xFFFF) as u16) as i16 as i64;
    if value < -COMPONENT_MAX {
        -COMPONENT_MAX
    } else {
        value
    }
}

/// A rotation packed into 64 bits: **~0.0017 deg mean, ~0.0034 deg max**.
///
/// Four [`Signed16`] SNORM components, packed low to high as `x`, `y`, `z`,
/// `w`. That is 2.3x inside the 1/128 deg (0.0078 deg) target with no chart and no
/// warp: at 64 bits the chart machinery stops paying for itself, because the
/// redundancy of storing four numbers for three degrees of freedom costs about
/// one bit and the budget is there.
///
/// # Where those figures come from
///
/// They are **extrapolated, not measured**. The source harness reports 0.4298 deg
/// mean / 0.8712 deg max for this encoding at 4x8 bits, and uniform quantization
/// error scales linearly with step size, so eight more bits per component
/// divides both by 256. The extrapolation is sound, but note where it lands:
/// the source paper states its own `f32` tables plateau near 0.001-0.01 deg, which
/// is exactly this range -- so the test that checks it must use an **`f64`**
/// reference, or it measures the harness rather than the codec.
///
/// # Sign canonicalization
///
/// The largest-magnitude component is forced positive, ties broken by lowest
/// index. Without this the double cover gives one rotation two bit patterns and
/// [`Hash`] and [`Eq`] would lie. Comparison and hashing route through
/// [`canonicalize`](Self::canonicalize), so a non-canonical pattern that
/// arrives over a wire still behaves as the rotation it denotes.
#[repr(transparent)]
#[derive(Clone, Copy)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "arbitrary", derive(::arbitrary::Arbitrary))]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct FineRotation(u64);

impl Default for FineRotation {
    #[inline]
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl FineRotation {
    /// The rotation that changes nothing: `(0, 0, 0, 1)`.
    pub const IDENTITY: Self = Self((COMPONENT_MAX as u64) << 48);

    /// Wraps a raw bit pattern.
    ///
    /// Every pattern decodes to a unit quaternion. Patterns that are not
    /// sign-canonical still compare and hash as the rotation they denote; see
    /// [`canonicalize`](Self::canonicalize).
    #[must_use]
    #[inline]
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// The raw bit pattern, exactly as stored.
    #[must_use]
    #[inline]
    pub const fn to_bits(self) -> u64 {
        self.0
    }

    /// The four components as raw `i16` values, in `x`, `y`, `z`, `w` order.
    ///
    /// `i16::MIN` is folded onto `-COMPONENT_MAX`, the way `Rotation` folds its
    /// own one-past-the-end field pattern. The signed-normalized convention
    /// keeps `+/-1` symmetric, so `MIN` is `-MAX` and the extra pattern denotes
    /// nothing new -- but left alone it makes `canonicalize` negate a value that
    /// wraps straight back to itself, so `canonicalize` would not be idempotent
    /// and [`is_canonical`](Self::is_canonical) would be false for its own
    /// output.
    #[inline]
    const fn components(self) -> [i64; 4] {
        [
            fold(self.0),
            fold(self.0 >> 16),
            fold(self.0 >> 32),
            fold(self.0 >> 48),
        ]
    }

    /// Packs four raw component values.
    #[inline]
    const fn from_components(c: [i64; 4]) -> Self {
        Self(
            ((c[0] as i16 as u16) as u64)
                | (((c[1] as i16 as u16) as u64) << 16)
                | (((c[2] as i16 as u16) as u64) << 32)
                | (((c[3] as i16 as u16) as u64) << 48),
        )
    }

    /// The canonical bit pattern for this rotation.
    ///
    /// Forces the largest-magnitude component positive, breaking ties by lowest
    /// index, so each rotation has exactly one encoding. Idempotent: it goes
    /// through the same component decode `to_versor` uses, so the one
    /// out-of-convention
    /// `i16::MIN` pattern is folded onto `-MAX` whether or not the sign flips.
    #[must_use]
    #[inline]
    pub const fn canonicalize(self) -> Self {
        if self.0 == 0 {
            // Names no rotation; `to_versor` reads it as the identity, so the
            // canonical encoding of it is the identity's.
            return Self::IDENTITY;
        }
        let c = self.components();
        let mut largest = 0;
        let mut i = 1;
        while i < 4 {
            if c[i].unsigned_abs() > c[largest].unsigned_abs() {
                largest = i;
            }
            i += 1;
        }
        if c[largest] < 0 {
            // Every component is now in `-MAX ..= MAX`, so negating stays in
            // range and cannot wrap back onto itself.
            Self::from_components([-c[0], -c[1], -c[2], -c[3]])
        } else {
            Self::from_components(c)
        }
    }

    /// Returns `true` if this is already the canonical encoding.
    #[must_use]
    #[inline]
    pub const fn is_canonical(self) -> bool {
        self.0 == self.canonicalize().0
    }

    /// Packs a unit quaternion.
    ///
    /// Normalizes, rounds each component onto the `Signed16` scale, and
    /// canonicalizes the sign.
    #[must_use]
    #[inline]
    pub const fn from_versor(q: Versor) -> Self {
        let unit = normalize4(q.bits());
        let mut out = [0i64; 4];
        let mut i = 0;
        while i < 4 {
            // Q30 to the SNORM scale, one rounding.
            let scaled = (unit[i] as i64) * COMPONENT_MAX;
            let rounded = round_shift_i64(scaled, 30);
            out[i] = if rounded > COMPONENT_MAX {
                COMPONENT_MAX
            } else if rounded < -COMPONENT_MAX {
                -COMPONENT_MAX
            } else {
                rounded
            };
            i += 1;
        }
        Self::from_components(out).canonicalize()
    }

    /// Unpacks into a unit quaternion.
    ///
    /// The four raw `i16`s go straight to the shared normalize -- which is
    /// scale-free, so there is no division by `32767` to perform first.
    ///
    /// The all-zero pattern -- what a zeroed buffer, a `serde` `0` or
    /// `bytemuck::zeroed` produces -- names no rotation at all, and
    /// `normalize4` reads it as the identity rather than handing back a zero
    /// quaternion.
    #[must_use]
    #[inline]
    pub const fn to_versor(self) -> Versor {
        Versor::from_xyzw_unchecked(normalize4(self.components()))
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

    /// Upgrades from the 32-bit tier. Total.
    ///
    /// Not lossless in the way "upgrade" suggests: this re-quantizes, adding up
    /// to this type's own 0.0034 deg on top of the 0.186 deg the
    /// [`Rotation`] already carries. That is a 1.8% increase in a quantity
    /// already dominated by the coarse codec -- not a free improvement.
    #[must_use]
    #[inline]
    pub const fn from_rotation(r: Rotation) -> Self {
        Self::from_versor(r.to_versor())
    }

    /// Downgrades to the 32-bit tier. Total, and loses accuracy to that tier.
    #[must_use]
    #[inline]
    pub const fn to_rotation(self) -> Rotation {
        Rotation::from_versor(self.to_versor())
    }

    /// The components as [`Signed16`] values, in `x`, `y`, `z`, `w` order.
    #[must_use]
    #[inline]
    pub const fn to_signed(self) -> [Signed16; 4] {
        let c = self.components();
        [
            Signed16::from_bits(c[0] as i16),
            Signed16::from_bits(c[1] as i16),
            Signed16::from_bits(c[2] as i16),
            Signed16::from_bits(c[3] as i16),
        ]
    }
}

impl PartialEq for FineRotation {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.canonicalize().0 == other.canonicalize().0
    }
}

impl Eq for FineRotation {}

impl core::hash::Hash for FineRotation {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.canonicalize().0.hash(state);
    }
}

impl From<Versor> for FineRotation {
    #[inline]
    fn from(q: Versor) -> Self {
        Self::from_versor(q)
    }
}

impl From<FineRotation> for Versor {
    #[inline]
    fn from(r: FineRotation) -> Self {
        r.to_versor()
    }
}

impl From<Basis> for FineRotation {
    #[inline]
    fn from(m: Basis) -> Self {
        Self::from_basis(m)
    }
}

impl From<FineRotation> for Basis {
    #[inline]
    fn from(r: FineRotation) -> Self {
        r.to_basis()
    }
}

impl From<Rotation> for FineRotation {
    #[inline]
    fn from(r: Rotation) -> Self {
        Self::from_rotation(r)
    }
}

impl From<FineRotation> for Rotation {
    #[inline]
    fn from(r: FineRotation) -> Self {
        r.to_rotation()
    }
}

impl core::fmt::Debug for FineRotation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "FineRotation({:#018x} -> {:?})",
            self.0,
            self.to_versor()
        )
    }
}
