//! A matrix carried with the power of two its entries were scaled by.

use corvid_fixed::I16F16;
use corvid_vector::FinePoint;

use crate::linear::{Linear3, bits, divide_shifted, narrow_shifted};

/// How near the top of an [`I16F16`] the largest entry of a scaled matrix is
/// put, as a bit position.
///
/// One bit below the width, so that a caller reading the entries out and
/// summing three products has the headroom to do it in the type rather than in
/// an accumulator.
const PEAK_BIT: u32 = 30;

/// The most a matrix's entries are scaled by.
///
/// Reached only by a matrix whose entries are near zero, where the scaling has
/// nothing left to recover. Bounded so that the products a scaled entry takes
/// part in stay inside an `i128` with room to spare.
const MAX_SHIFT: u32 = 32;

/// A [`Linear3`] whose entries are the map's own multiplied by `2^shift`.
///
/// A triangle's ECEF-to-local matrix has entries of about one over the length
/// of an edge, so on a six-hundred-metre face they are 0.0017 and an [`I16F16`]
/// holds them to eight bits. That is not a coordinate's worth of precision, and
/// it is the reason the eight-metre edge limit looked like a resolution limit:
/// it was propping up the matrices as much as the codes. Scaling the entries up
/// to fill their width and carrying the shift beside them takes the precision
/// back -- fifteen bits of it, whatever the triangle's size -- and costs one
/// `u32` per face and one shift per multiply.
///
/// The scale is chosen once, when the matrix is inverted, so that the largest
/// entry lands just under `2^30` of an [`I16F16`]'s bit pattern. Every
/// operation here answers in the *unscaled* value, so a caller never sees the
/// shift unless it asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Scaled3 {
    matrix: Linear3,
    shift: u32,
}

impl Scaled3 {
    /// The identity, at no scale at all.
    pub const IDENTITY: Self = Self {
        matrix: Linear3::IDENTITY,
        shift: 0,
    };

    /// A matrix and the power of two its entries were multiplied by.
    #[must_use]
    #[inline]
    pub const fn new(matrix: Linear3, shift: u32) -> Self {
        Self { matrix, shift }
    }

    /// The stored entries, which are the map's own scaled up.
    #[must_use]
    #[inline]
    pub const fn matrix(self) -> Linear3 {
        self.matrix
    }

    /// How many bits the entries were scaled up by.
    #[must_use]
    #[inline]
    pub const fn shift(self) -> u32 {
        self.shift
    }

    /// The three rows of the stored matrix.
    ///
    /// Scaled, and uniformly so, which is why
    /// [`resolve_wall`](crate::calc_next_nav_tri) can normalize one and get the
    /// direction it wanted: a scale that is the same in all three components is
    /// a scale a unit vector does not have.
    #[must_use]
    #[inline]
    pub const fn rows(self) -> [FinePoint; 3] {
        self.matrix.rows()
    }

    /// Maps a vector, rounding once at the end.
    ///
    /// The three products are summed at full width and the scale comes off in
    /// the same rounding that narrows them, so the answer is the representable
    /// value nearest the exact one and the scaling has cost nothing.
    #[must_use]
    #[inline]
    pub const fn apply(self, vector: FinePoint) -> FinePoint {
        let columns = self.matrix.columns();
        let [a, b, c] = [bits(columns[0]), bits(columns[1]), bits(columns[2])];
        let v = bits(vector);
        FinePoint::new(
            narrow_shifted(a[0] * v[0] + b[0] * v[1] + c[0] * v[2], self.shift),
            narrow_shifted(a[1] * v[0] + b[1] * v[1] + c[1] * v[2], self.shift),
            narrow_shifted(a[2] * v[0] + b[2] * v[1] + c[2] * v[2], self.shift),
        )
    }

    /// `self * rhs`, answered unscaled: the map that applies `rhs` and then
    /// this one.
    ///
    /// What a seam is built out of. The product of an inverse and a frame is a
    /// matrix of about unit entries whichever sizes the two triangles are, so
    /// the answer needs no scale of its own -- and taking the scale off here,
    /// after the full-width products, is what makes a seam between two large
    /// triangles as exact as one between two small ones.
    #[must_use]
    #[inline]
    pub const fn compose(self, rhs: Linear3) -> Linear3 {
        let columns = rhs.columns();
        Linear3::from_columns([
            self.apply(columns[0]),
            self.apply(columns[1]),
            self.apply(columns[2]),
        ])
    }

    /// The inverse of a matrix, scaled so its entries fill their width.
    ///
    /// [`None`] if the determinant is zero, which is the one thing an inverse
    /// cannot answer. The rows are the cross products of the columns over the
    /// determinant, exactly as [`Linear3::inverse`] computes them, with the
    /// shift folded into the one division each entry takes so that nothing is
    /// rounded twice.
    #[must_use]
    pub(crate) const fn inverse_of(matrix: Linear3) -> Option<Self> {
        let det = matrix.determinant();
        if det == 0 {
            return None;
        }
        let columns = matrix.columns();
        let [a, b, c] = [bits(columns[0]), bits(columns[1]), bits(columns[2])];
        let rows = [cross(b, c), cross(c, a), cross(a, b)];

        let mut peak = 0u128;
        let mut row = 0;
        while row < 3 {
            let mut axis = 0;
            while axis < 3 {
                let size = rows[row][axis].unsigned_abs();
                if size > peak {
                    peak = size;
                }
                axis += 1;
            }
            row += 1;
        }
        let shift = headroom(peak, det.unsigned_abs());

        Some(Self {
            matrix: Linear3::from_rows([
                scaled_row(rows[0], det, shift),
                scaled_row(rows[1], det, shift),
                scaled_row(rows[2], det, shift),
            ]),
            shift,
        })
    }
}

impl Default for Scaled3 {
    #[inline]
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// How far the largest entry of an inverse can be scaled up before it fills an
/// [`I16F16`].
#[inline]
const fn headroom(peak: u128, det: u128) -> u32 {
    let entry = (peak << 32) / det;
    let used = 128 - entry.leading_zeros();
    if used >= PEAK_BIT {
        0
    } else if PEAK_BIT - used > MAX_SHIFT {
        MAX_SHIFT
    } else {
        PEAK_BIT - used
    }
}

/// One row of a scaled inverse.
#[inline]
const fn scaled_row(cross: [i128; 3], det: i128, shift: u32) -> FinePoint {
    FinePoint::new(
        divide_shifted(cross[0], det, shift),
        divide_shifted(cross[1], det, shift),
        divide_shifted(cross[2], det, shift),
    )
}

/// `a x b`, at the width the bit patterns are already in.
#[inline]
const fn cross(a: [i128; 3], b: [i128; 3]) -> [i128; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The number of fractional bits a [`FinePoint`] entry holds, which is what a
/// shift of zero means.
const _: () = assert!(I16F16::ONE.to_bits() == 1 << 16);
