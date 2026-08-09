//! The matrix algebra: building a basis, checking it is one, reading its
//! columns back, and the operations that take bases to bases.

use corvid_fixed::I2F30;
use corvid_vector::Direction;

use super::{Basis, ONE, ORTHONORMAL_TOLERANCE, entry, signed_from_q30};

impl Basis {
    /// The rotation that changes nothing: facing **+Y** with **+Z** up.
    ///
    /// Exact, because `1.0` is exactly `2^30` in [`I2F30`].
    pub const IDENTITY: Self = Self {
        rows: [
            [I2F30::ONE, I2F30::ZERO, I2F30::ZERO],
            [I2F30::ZERO, I2F30::ONE, I2F30::ZERO],
            [I2F30::ZERO, I2F30::ZERO, I2F30::ONE],
        ],
    };

    /// Builds a basis from nine entries, or `None` if they are not a rotation.
    ///
    /// Accepts only matrices whose rows are orthonormal to within a
    /// quantization tolerance -- about `1.5e-5`, loose enough for a rotation
    /// that arrived over a wire as `f32` -- *and* whose determinant is `+1`,
    /// which is what rejects reflections. This is the deserialization and FFI door;
    /// everything inside the crate reaches a `Basis` through a type that is
    /// already known to be a rotation.
    #[must_use]
    #[inline]
    pub const fn from_rows(rows: [[I2F30; 3]; 3]) -> Option<Self> {
        let candidate = Self { rows };
        if candidate.is_orthonormal() && candidate.has_unit_determinant() {
            Some(candidate)
        } else {
            None
        }
    }

    /// Builds a basis from entries already known to be a rotation.
    #[inline]
    pub(crate) const fn from_rows_unchecked(rows: [[I2F30; 3]; 3]) -> Self {
        Self { rows }
    }

    /// The nine entries, row-major.
    #[must_use]
    #[inline]
    pub const fn to_rows(self) -> [[I2F30; 3]; 3] {
        self.rows
    }

    /// The dot product of two rows, at Q30.
    ///
    /// Accumulated at `i128`. This runs *before* the orthonormality test has
    /// passed, so it sees the very matrices [`from_rows`](Self::from_rows)
    /// exists to reject: three products of `I2F30::MAX` reach `1.38e19`, past
    /// `i64::MAX`, and the check would panic on hostile input rather than
    /// return `None`. The result is clamped back into `i64`, which loses
    /// nothing for a matrix anywhere near orthonormal and keeps a wild one
    /// safely outside the tolerance window.
    #[inline]
    const fn row_dot(self, a: usize, b: usize) -> i64 {
        let rows = self.rows;
        let sum = ((rows[a][0].to_bits() as i128) * (rows[b][0].to_bits() as i128)
            + (rows[a][1].to_bits() as i128) * (rows[b][1].to_bits() as i128)
            + (rows[a][2].to_bits() as i128) * (rows[b][2].to_bits() as i128))
            >> 30;
        if sum > i64::MAX as i128 {
            i64::MAX
        } else if sum < i64::MIN as i128 {
            i64::MIN
        } else {
            sum as i64
        }
    }

    /// Returns `true` if every row is a unit vector and every pair is
    /// perpendicular.
    #[inline]
    const fn is_orthonormal(self) -> bool {
        let mut i = 0;
        while i < 3 {
            if (self.row_dot(i, i).saturating_sub(ONE)).abs() > ORTHONORMAL_TOLERANCE {
                return false;
            }
            let mut j = i + 1;
            while j < 3 {
                // `saturating_abs`: a wild row `j` whose own diagonal test has
                // not run yet can drive this to `i64::MIN`, and plain `abs`
                // would overflow on it.
                if self.row_dot(i, j).saturating_abs() > ORTHONORMAL_TOLERANCE {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// Returns `true` if the determinant is `+1`, which rules out reflections.
    ///
    /// Orthonormality alone leaves both `+1` and `-1`, and a `-1` matrix is a
    /// reflection: it would satisfy the `i64` bound but flip handedness, so
    /// `right = forward x up` would stop holding.
    #[inline]
    const fn has_unit_determinant(self) -> bool {
        let m = self.rows;
        // Entries are Q30, so a cofactor comes back at Q60 and the determinant
        // at Q90 -- hence `i128` and a 60-bit shift back to Q30.
        let cofactor_a = (m[1][1].to_bits() as i64) * (m[2][2].to_bits() as i64)
            - (m[1][2].to_bits() as i64) * (m[2][1].to_bits() as i64);
        let cofactor_b = (m[1][0].to_bits() as i64) * (m[2][2].to_bits() as i64)
            - (m[1][2].to_bits() as i64) * (m[2][0].to_bits() as i64);
        let cofactor_c = (m[1][0].to_bits() as i64) * (m[2][1].to_bits() as i64)
            - (m[1][1].to_bits() as i64) * (m[2][0].to_bits() as i64);

        let determinant = ((m[0][0].to_bits() as i128) * (cofactor_a as i128)
            - (m[0][1].to_bits() as i128) * (cofactor_b as i128)
            + (m[0][2].to_bits() as i128) * (cofactor_c as i128))
            >> 60;
        (determinant - ONE as i128).abs() <= ORTHONORMAL_TOLERANCE as i128
    }

    /// The local **+X** axis in world space: rightward. Column 0.
    #[must_use]
    #[inline]
    pub const fn right(self) -> Direction {
        self.column(0)
    }

    /// The local **+Y** axis in world space: forward. Column 1.
    #[must_use]
    #[inline]
    pub const fn forward(self) -> Direction {
        self.column(1)
    }

    /// The local **+Z** axis in world space: upward. Column 2.
    #[must_use]
    #[inline]
    pub const fn up(self) -> Direction {
        self.column(2)
    }

    /// One column, as a unit direction.
    #[inline]
    const fn column(self, index: usize) -> Direction {
        Direction::new(
            signed_from_q30(self.rows[0][index].to_bits()),
            signed_from_q30(self.rows[1][index].to_bits()),
            signed_from_q30(self.rows[2][index].to_bits()),
        )
    }

    /// The inverse rotation, which for an orthonormal matrix is the transpose.
    ///
    /// Free -- nine moves and no arithmetic -- which is why untransforming a
    /// point costs exactly what transforming one costs.
    #[must_use]
    #[inline]
    pub const fn inverse(self) -> Self {
        let m = self.rows;
        Self {
            rows: [
                [m[0][0], m[1][0], m[2][0]],
                [m[0][1], m[1][1], m[2][1]],
                [m[0][2], m[1][2], m[2][2]],
            ],
        }
    }

    /// Composes two rotations, applying `rhs` **first**, then `self`.
    ///
    /// Matrix multiplication order, and `glam`'s `Mul`. Covered by a test that
    /// fails if the order is ever flipped.
    #[must_use]
    #[inline]
    pub const fn compose(self, rhs: Self) -> Self {
        let (a, b) = (self.rows, rhs.rows);
        let mut out = [[I2F30::ZERO; 3]; 3];
        let mut i = 0;
        while i < 3 {
            let mut j = 0;
            while j < 3 {
                // Three Q30 products, each at most `2^60`, summed: `2^61.6`.
                let sum = (a[i][0].to_bits() as i64) * (b[0][j].to_bits() as i64)
                    + (a[i][1].to_bits() as i64) * (b[1][j].to_bits() as i64)
                    + (a[i][2].to_bits() as i64) * (b[2][j].to_bits() as i64);
                out[i][j] = entry(sum);
                j += 1;
            }
            i += 1;
        }
        Self { rows: out }
    }

    /// Returns `true` if every entry is within `tolerance` of `other`'s.
    #[must_use]
    #[inline]
    pub const fn abs_diff_eq(self, other: Self, tolerance: I2F30) -> bool {
        let limit = tolerance.to_bits() as i64;
        let mut i = 0;
        while i < 3 {
            let mut j = 0;
            while j < 3 {
                let difference =
                    self.rows[i][j].to_bits() as i64 - other.rows[i][j].to_bits() as i64;
                if difference.abs() > limit {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}
