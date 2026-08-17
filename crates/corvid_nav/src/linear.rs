//! The two matrices a triangle's frame is made of.
//!
//! `corvid_transform`'s [`Transform`] is rigid, and a barycentric frame is not:
//! its first two axes are the triangle's own edges, which are neither unit
//! length nor orthogonal to one another. So the maps live here, in the crate
//! whose subject matter they are, rather than pushing a general affine into a
//! crate whose whole promise is that a transform preserves distance.
//!
//! Every entry is an [`I16F16`]. That is 15.26 um over +/-32.7 km, which covers
//! both an edge vector of a triangle at most [`MAX_EDGE`](crate::MAX_EDGE)
//! across and the reciprocal entries of its inverse down to a millimetre-wide
//! sliver.
//!
//! [`Transform`]: https://docs.rs/corvid_transform/latest/corvid_transform/struct.Transform.html

use corvid_fixed::I16F16;
use corvid_vector::FinePoint;

/// Rounds a `Q32` product back to `Q16` and clamps it into [`I16F16`].
///
/// Halfway cases round away from zero, so that negating an input negates the
/// output exactly. An asymmetric rounding here would make a mirrored level
/// diverge from its reflection.
#[inline]
const fn narrow_q32(value: i128) -> I16F16 {
    const HALF: i128 = 1 << 15;
    clamp_q16(if value >= 0 {
        (value + HALF) >> 16
    } else {
        -((-value + HALF) >> 16)
    })
}

/// Clamps a `Q16` bit pattern into [`I16F16`].
#[inline]
const fn clamp_q16(bits: i128) -> I16F16 {
    let limit = I16F16::MAX.to_bits() as i128;
    if bits > limit {
        I16F16::MAX
    } else if bits < -limit {
        I16F16::MIN
    } else {
        I16F16::from_bits(bits as i32)
    }
}

/// `numerator * 2^32 / divisor`, which is the `Q16` bit pattern of a `Q32`
/// value over a `Q48` one.
///
/// Rounded away from zero at the halfway point and clamped, so a near-singular
/// matrix saturates rather than wrapping into an inverse that points the wrong
/// way.
#[inline]
const fn divide_q16(numerator: i128, divisor: i128) -> I16F16 {
    let scaled = numerator << 32;
    let magnitude = divisor.unsigned_abs();
    let rounded = (scaled.unsigned_abs() + magnitude / 2) / magnitude;
    if (scaled < 0) == (divisor < 0) {
        clamp_q16(rounded as i128)
    } else {
        clamp_q16(-(rounded as i128))
    }
}

/// The three bit patterns of a vector, as the width the products need.
#[inline]
const fn bits(vector: FinePoint) -> [i128; 3] {
    [
        vector.x().to_bits() as i128,
        vector.y().to_bits() as i128,
        vector.z().to_bits() as i128,
    ]
}

/// `a x b`, at `Q32` and exact.
///
/// Wider than a [`FinePoint`] cross product, which would round back to `Q16`
/// and saturate. What the extra width is for is a normal: only the ratios of
/// the three components matter to
/// [`Direction::from_ratio`](corvid_vector::Direction::from_ratio), and giving
/// it the exact ones is free.
#[must_use]
#[inline]
pub(crate) const fn cross_bits(a: FinePoint, b: FinePoint) -> [i128; 3] {
    cross_q32(bits(a), bits(b))
}

/// `a x b`, at `Q32` and exact.
#[inline]
const fn cross_q32(a: [i128; 3], b: [i128; 3]) -> [i128; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A 3x3 matrix over [`I16F16`], held as its three columns.
///
/// Column `j` is where the local axis `j` lands, which is the order the
/// triangle's own geometry supplies: the first two columns of a local-to-ECEF
/// matrix are the two edge vectors and the third is the up direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Linear3 {
    columns: [FinePoint; 3],
}

impl Linear3 {
    /// The matrix that changes nothing.
    pub const IDENTITY: Self = Self {
        columns: [
            FinePoint::new(I16F16::ONE, I16F16::ZERO, I16F16::ZERO),
            FinePoint::new(I16F16::ZERO, I16F16::ONE, I16F16::ZERO),
            FinePoint::new(I16F16::ZERO, I16F16::ZERO, I16F16::ONE),
        ],
    };

    /// Builds a matrix from where each of the three local axes lands.
    #[must_use]
    #[inline]
    pub const fn from_columns(columns: [FinePoint; 3]) -> Self {
        Self { columns }
    }

    /// Builds a matrix from its three rows.
    #[must_use]
    #[inline]
    pub const fn from_rows(rows: [FinePoint; 3]) -> Self {
        Self {
            columns: [
                FinePoint::new(rows[0].x(), rows[1].x(), rows[2].x()),
                FinePoint::new(rows[0].y(), rows[1].y(), rows[2].y()),
                FinePoint::new(rows[0].z(), rows[1].z(), rows[2].z()),
            ],
        }
    }

    /// Where each of the three local axes lands.
    #[must_use]
    #[inline]
    pub const fn columns(self) -> [FinePoint; 3] {
        self.columns
    }

    /// The three rows.
    ///
    /// A row of an ECEF-to-local matrix is the plane normal of a local
    /// coordinate: row 0 is orthogonal to both the second edge vector and the
    /// up direction, which is exactly the outward normal of a vertical wall
    /// standing on the edge those two span. [`NavTri`](crate::NavTri) bounces
    /// off an unwalkable edge with it, and pays nothing to have it.
    #[must_use]
    #[inline]
    pub const fn rows(self) -> [FinePoint; 3] {
        self.transposed().columns
    }

    /// The transpose.
    #[must_use]
    #[inline]
    pub const fn transposed(self) -> Self {
        let [a, b, c] = self.columns;
        Self {
            columns: [
                FinePoint::new(a.x(), b.x(), c.x()),
                FinePoint::new(a.y(), b.y(), c.y()),
                FinePoint::new(a.z(), b.z(), c.z()),
            ],
        }
    }

    /// Maps a vector, rounding once at the end.
    ///
    /// The three products are summed at full width before the single rounding,
    /// so the answer is the representable value nearest the exact one rather
    /// than three roundings deep.
    #[must_use]
    #[inline]
    pub const fn apply(self, vector: FinePoint) -> FinePoint {
        let [a, b, c] = [
            bits(self.columns[0]),
            bits(self.columns[1]),
            bits(self.columns[2]),
        ];
        let v = bits(vector);
        FinePoint::new(
            narrow_q32(a[0] * v[0] + b[0] * v[1] + c[0] * v[2]),
            narrow_q32(a[1] * v[0] + b[1] * v[1] + c[1] * v[2]),
            narrow_q32(a[2] * v[0] + b[2] * v[1] + c[2] * v[2]),
        )
    }

    /// `self * rhs`: the map that applies `rhs` and then `self`.
    #[must_use]
    #[inline]
    pub const fn compose(self, rhs: Self) -> Self {
        Self {
            columns: [
                self.apply(rhs.columns[0]),
                self.apply(rhs.columns[1]),
                self.apply(rhs.columns[2]),
            ],
        }
    }

    /// The determinant, at `Q48` and exact.
    ///
    /// Wider than the matrix's own scale on purpose. For a triangle's local
    /// frame this is `2 * area * cos(slope)` in square metres, and both of the
    /// facts a caller wants from it -- has the triangle any area, and is it
    /// flat enough to have a height axis -- are questions about how near zero
    /// it is, which a saturating narrow answer would destroy.
    #[must_use]
    #[inline]
    pub const fn determinant(self) -> i128 {
        let a = bits(self.columns[0]);
        let cross = cross_q32(bits(self.columns[1]), bits(self.columns[2]));
        a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2]
    }

    /// The inverse, or [`None`] if the determinant is zero.
    ///
    /// The rows of the inverse are the cross products of the columns, divided
    /// by the determinant -- one division per entry, from an exact `Q64`
    /// numerator, so nothing is rounded twice. A near-singular matrix inverts
    /// to entries that saturate rather than wrap;
    /// [`NavTri`](crate::NavTri) rejects such a triangle before it gets here.
    #[must_use]
    #[inline]
    pub const fn inverse(self) -> Option<Self> {
        let det = self.determinant();
        if det == 0 {
            return None;
        }
        let [a, b, c] = [
            bits(self.columns[0]),
            bits(self.columns[1]),
            bits(self.columns[2]),
        ];
        Some(Self::from_rows([
            Self::scaled_row(cross_q32(b, c), det),
            Self::scaled_row(cross_q32(c, a), det),
            Self::scaled_row(cross_q32(a, b), det),
        ]))
    }

    /// One row of an inverse: a `Q32` cross product over a `Q48` determinant,
    /// expressed back at `Q16`.
    #[inline]
    const fn scaled_row(cross: [i128; 3], det: i128) -> FinePoint {
        FinePoint::new(
            divide_q16(cross[0], det),
            divide_q16(cross[1], det),
            divide_q16(cross[2], det),
        )
    }
}

impl Default for Linear3 {
    #[inline]
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// A [`Linear3`] and a translation: the map that carries a position from one
/// triangle's local frame into another's.
///
/// Both frames are local, so both ends are near-field and the translation is a
/// [`FinePoint`] rather than a world position. Two neighbouring triangles put
/// their origins at most twice [`MAX_EDGE`](crate::MAX_EDGE) apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Affine3 {
    linear: Linear3,
    translation: FinePoint,
}

impl Affine3 {
    /// Builds an affine map from its linear part and its translation.
    #[must_use]
    #[inline]
    pub const fn new(linear: Linear3, translation: FinePoint) -> Self {
        Self {
            linear,
            translation,
        }
    }

    /// The linear part, which is what carries a velocity.
    #[must_use]
    #[inline]
    pub const fn linear(self) -> Linear3 {
        self.linear
    }

    /// The translation, which is what a velocity does not get.
    #[must_use]
    #[inline]
    pub const fn translation(self) -> FinePoint {
        self.translation
    }

    /// Maps a position.
    #[must_use]
    #[inline]
    pub const fn apply(self, point: FinePoint) -> FinePoint {
        self.linear.apply(point).add(self.translation)
    }

    /// Maps a velocity, which is the linear part alone.
    #[must_use]
    #[inline]
    pub const fn apply_vector(self, vector: FinePoint) -> FinePoint {
        self.linear.apply(vector)
    }
}
