//! The quaternion algebra: building a versor, checking it is one, and the
//! operations that take versors to versors.

use corvid_fixed::I2F30;

use super::{ONE, UNIT_TOLERANCE, Versor};
use crate::basis::entry;
use crate::normalize::{normalize4, normalize4_fast};

impl Versor {
    /// The rotation that changes nothing: `(0, 0, 0, 1)`.
    pub const IDENTITY: Self = Self {
        components: [I2F30::ZERO, I2F30::ZERO, I2F30::ZERO, I2F30::ONE],
    };

    /// Builds a versor from four components, or `None` if they are not unit.
    ///
    /// Unit-checked for the same reason [`Basis::from_rows`](crate::Basis::from_rows) is orthonormality-
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
    /// of `1.5` -- a legal `I2F30` -- already overflow. Clamping back into `i64`
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
    /// lands within a step of `1.0` -- where the approximation happens to be
    /// exact. The consequence is not the error you would expect from `3.2e-5`;
    /// it is a **deadband**. The scale factor is quantized at `2^-15`, so any
    /// drift finer than that is invisible and passes through untouched, and the
    /// versor settles at the edge of the deadband rather than on the sphere.
    ///
    /// A million composes, each followed by a renormalize, measured:
    ///
    /// | | worst `abs(||q||^2 - 1)` |
    /// |---|---|
    /// | no renormalize | `8.3e-4` |
    /// | `renormalize_fast` | `1.5e-5` |
    /// | [`renormalize`](Self::renormalize) | `3.7e-9` |
    ///
    /// So it does bound the drift -- it does not diverge -- but it bounds it four
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
        // a representable value -- and reachable through `bytemuck` -- whose
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
    /// The double cover: `q` and `-q` name one rotation, which is why the
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
    /// composition in the crate -- a [`Basis`](crate::Basis)'s costs twenty-seven multiplies.
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
    /// Compares components, not rotations: `q` and `-q` are the same rotation
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
}
