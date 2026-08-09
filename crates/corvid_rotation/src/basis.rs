//! [`Basis`]: a rotation as a 3x3 matrix of [`I2F30`] entries.

use corvid_fixed::{I2F30, I16F16, I24F8, I48F16, Signed32};
use corvid_vector::{Direction, FinePoint, GlobalFinePoint, GlobalPoint};

/// `1.0` at the Q30 scale the entries use.
const ONE: i64 = 1 << 30;

/// How far from orthonormal [`Basis::from_rows`] will still accept, in Q30
/// last bits.
///
/// A rotation built in `f64` and rounded into `I2F30` lands within a handful of
/// last bits; `2^14` -- about `1.5e-5` -- leaves room for one that arrived over a
/// wire as `f32` while still rejecting anything that is not a rotation. The
/// point of the check is to keep the [`i64` invariant](crate#the-i64-invariant)
/// true, not to police the last bit.
const ORTHONORMAL_TOLERANCE: i64 = 1 << 14;

/// A rotation as a 3x3 matrix of [`I2F30`] entries: 36 bytes.
///
/// Rotating a point is 9 multiplies, 6 adds and 3 shifts, and the inverse is
/// the transpose -- so untransforming costs exactly what transforming costs.
/// That is what makes this the type to reach for when many points go through
/// one rotation, which is the earth-scale VR case. [`Versor`](crate::Versor)
/// is 16 bytes and composes more cheaply; the crate's benchmark says which to
/// prefer per operation.
///
/// # Storage and convention
///
/// Row-major, with `rotate(v)[i] = sum over j of rows[i][j] * v[j]`. The
/// world-space image of a local axis is therefore a **column**: [`right`](Self::right) is column 0,
/// [`forward`](Self::forward) is column 1, [`up`](Self::up) is column 2, in the
/// crate's right-handed **+X right, +Y forward, +Z up** convention.
/// [`IDENTITY`](Self::IDENTITY) faces +Y with +Z up, so an identity transform
/// looks forward rather than at the floor.
///
/// # Rows must be orthonormal
///
/// The `i64` bound the hot path relies on holds **only for orthonormal rows**:
/// the row sum is bounded by Cauchy-Schwarz against a unit row, and a longer
/// row lifts it past `i64::MAX`. So the ordinary way in is a type already known
/// to be a rotation, and [`from_rows`](Self::from_rows) -- which exists for
/// deserialization and FFI -- verifies orthonormality and a determinant of `+1`
/// before handing one back.
///
/// **The `bytemuck` feature is a second door, and it is not checked.** `Pod`
/// makes any 36 bytes a `Basis`, which is the point of the feature and also
/// bypasses [`from_rows`](Self::from_rows) entirely. A `Basis` assembled that
/// way from rows longer than one overflows `i64` inside
/// [`rotate_fine`](Self::rotate_fine) and [`compose`](Self::compose) -- a panic
/// under `overflow-checks`, a wrapped value without them. Put bytes through
/// [`from_rows`](Self::from_rows), or through
/// [`Rotation`](crate::Rotation)/[`FineRotation`](crate::FineRotation), whose
/// every bit pattern is a valid rotation by construction.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct Basis {
    rows: [[I2F30; 3]; 3],
}

impl Default for Basis {
    #[inline]
    fn default() -> Self {
        Self::IDENTITY
    }
}

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

/// `round(value / 2^shift)`, half away from zero.
#[inline]
pub(crate) const fn round_shift_i64(value: i64, shift: u32) -> i64 {
    let half = 1i64 << (shift - 1);
    if value >= 0 {
        (value + half) >> shift
    } else {
        -((-value + half) >> shift)
    }
}

/// `round(value / 2^shift)`, half away from zero, at `i128` width.
#[inline]
const fn round_shift_i128(value: i128, shift: u32) -> i128 {
    let half = 1i128 << (shift - 1);
    if value >= 0 {
        (value + half) >> shift
    } else {
        -((-value + half) >> shift)
    }
}

/// Clamps a Q30 value into `[-1, 1]`, for the trig entry points.
#[inline]
pub(crate) const fn clamp_q30(bits: i64) -> i64 {
    if bits > ONE {
        ONE
    } else if bits < -ONE {
        -ONE
    } else {
        bits
    }
}

/// One matrix entry: a Q60 intermediate rounded into [`I2F30`], saturating.
///
/// A rotation's entries live in `[-1, 1]` and never come near the clamp. The
/// clamp is here because the alternative -- a bare `as i32` -- *wraps*, and a
/// wrapped entry reads as a plausible rotation of the opposite sign rather
/// than as the garbage it is. Saturating keeps the failure legible for input
/// that never went through [`Basis::from_rows`] or `Versor::from_xyzw`.
#[inline]
pub(crate) const fn entry(value: i64) -> I2F30 {
    let rounded = round_shift_i64(value, 30);
    if rounded > I2F30::MAX.to_bits() as i64 {
        I2F30::MAX
    } else if rounded < I2F30::MIN.to_bits() as i64 {
        I2F30::MIN
    } else {
        I2F30::from_bits(rounded as i32)
    }
}

/// Converts a Q30 value in `[-1, 1]` into a [`Signed32`], rounded and clamped.
#[inline]
pub(crate) const fn signed_from_q30(bits: i32) -> Signed32 {
    let scaled = (bits as i64) * (Signed32::MAX.to_bits() as i64);
    let rounded = round_shift_i64(scaled, 30);
    let limit = Signed32::MAX.to_bits() as i64;
    if rounded > limit {
        Signed32::MAX
    } else if rounded < -limit {
        Signed32::MIN
    } else {
        Signed32::from_bits(rounded as i32)
    }
}

impl Basis {
    /// Rotates a near-field offset.
    ///
    /// `i32 x i32 -> i64` throughout. The row sum is bounded by
    /// `sqrt(3) * 2^30 * 2^31 = 3.99e18` against `i64::MAX`'s 9.22e18 -- a 131%
    /// margin -- and the bound is Cauchy-Schwarz, `|m*v| <= |m||v|` with
    /// `|m| = 1`, so it holds **only because basis rows are unit-length**.
    /// That is what [`from_rows`](Self::from_rows) exists to guarantee. Partial
    /// sums obey the same bound with `sqrt(2)` in place of `sqrt(3)`, so the accumulation
    /// order is free.
    ///
    /// The *result* can still leave [`FinePoint`]'s range, because a rotation
    /// can map a corner of the cube onto an axis. This form saturates;
    /// [`checked_rotate_fine`](Self::checked_rotate_fine) reports it.
    #[must_use]
    #[inline]
    pub const fn rotate_fine(self, v: FinePoint) -> FinePoint {
        let m = self.rows;
        let [x, y, z] = v.to_array();
        FinePoint::new(
            i16f16_from_row(m[0], x, y, z),
            i16f16_from_row(m[1], x, y, z),
            i16f16_from_row(m[2], x, y, z),
        )
    }

    /// Rotates a near-field offset, or `None` if the result leaves range.
    #[must_use]
    #[inline]
    pub const fn checked_rotate_fine(self, v: FinePoint) -> Option<FinePoint> {
        let m = self.rows;
        let [x, y, z] = v.to_array();
        match (
            checked_i16f16_from_row(m[0], x, y, z),
            checked_i16f16_from_row(m[1], x, y, z),
            checked_i16f16_from_row(m[2], x, y, z),
        ) {
            (Some(x), Some(y), Some(z)) => Some(FinePoint::new(x, y, z)),
            _ => None,
        }
    }

    /// The inverse rotation of a near-field offset, by the transpose.
    #[must_use]
    #[inline]
    pub const fn unrotate_fine(self, v: FinePoint) -> FinePoint {
        self.inverse().rotate_fine(v)
    }

    /// Rotates a near-field offset into a world-scale one, without saturating.
    ///
    /// The `i64` bound says the row sum reaches at most `sqrt(3) * 2^30 * 2^31`, so
    /// the *result* can be up to `sqrt(3) x` longer than [`FinePoint`] holds even
    /// though the accumulation never overflows. Widening the output rather than
    /// the input keeps the arithmetic at `i32 x i32 -> i64` and loses nothing --
    /// which is what lets `corvid_transform`'s local->world conversion stay off
    /// the `i128` path that [`rotate_global_fine`](Self::rotate_global_fine)
    /// takes.
    ///
    /// Both types carry 16 fractional bits, so no rescaling happens either.
    #[must_use]
    #[inline]
    pub const fn rotate_fine_wide(self, v: FinePoint) -> GlobalFinePoint {
        let m = self.rows;
        let [x, y, z] = v.to_array();
        GlobalFinePoint::new(
            i48f16_from_fine_row(m[0], x, y, z),
            i48f16_from_fine_row(m[1], x, y, z),
            i48f16_from_fine_row(m[2], x, y, z),
        )
    }

    /// The inverse rotation of a near-field offset, or `None` if out of range.
    #[must_use]
    #[inline]
    pub const fn checked_unrotate_fine(self, v: FinePoint) -> Option<FinePoint> {
        self.inverse().checked_rotate_fine(v)
    }

    /// Rotates an object-scale offset. `i32 x i32 -> i64`, as
    /// [`rotate_fine`](Self::rotate_fine).
    #[must_use]
    #[inline]
    pub const fn rotate_global(self, v: GlobalPoint) -> GlobalPoint {
        let m = self.rows;
        let [x, y, z] = v.to_array();
        GlobalPoint::new(
            i24f8_from_row(m[0], x, y, z),
            i24f8_from_row(m[1], x, y, z),
            i24f8_from_row(m[2], x, y, z),
        )
    }

    /// Rotates an object-scale offset, or `None` if the result leaves range.
    #[must_use]
    #[inline]
    pub const fn checked_rotate_global(self, v: GlobalPoint) -> Option<GlobalPoint> {
        let m = self.rows;
        let [x, y, z] = v.to_array();
        match (
            checked_i24f8_from_row(m[0], x, y, z),
            checked_i24f8_from_row(m[1], x, y, z),
            checked_i24f8_from_row(m[2], x, y, z),
        ) {
            (Some(x), Some(y), Some(z)) => Some(GlobalPoint::new(x, y, z)),
            _ => None,
        }
    }

    /// The inverse rotation of an object-scale offset.
    #[must_use]
    #[inline]
    pub const fn unrotate_global(self, v: GlobalPoint) -> GlobalPoint {
        self.inverse().rotate_global(v)
    }

    /// The inverse rotation of an object-scale offset, or `None` if out of range.
    #[must_use]
    #[inline]
    pub const fn checked_unrotate_global(self, v: GlobalPoint) -> Option<GlobalPoint> {
        self.inverse().checked_rotate_global(v)
    }

    /// Rotates a world-scale offset. **The documented slow path.**
    ///
    /// `i64 x i32 -> i128`, because the operand is 64 bits wide. The fast
    /// pattern subtracts first and rotates the near-field difference, which is
    /// what `corvid_transform`'s world->local conversions do.
    #[must_use]
    #[inline]
    pub const fn rotate_global_fine(self, v: GlobalFinePoint) -> GlobalFinePoint {
        let m = self.rows;
        let [x, y, z] = v.to_array();
        GlobalFinePoint::new(
            i48f16_from_row(m[0], x, y, z),
            i48f16_from_row(m[1], x, y, z),
            i48f16_from_row(m[2], x, y, z),
        )
    }

    /// Rotates a world-scale offset, or `None` if the result leaves range.
    ///
    /// The wide tier's `checked` form. A rotation can make an in-range offset
    /// up to `sqrt(3) x` longer, and `I48F16` has no wider type to widen into -- so
    /// this is the only way to tell a saturated answer from a real one at
    /// world scale.
    #[must_use]
    #[inline]
    pub const fn checked_rotate_global_fine(self, v: GlobalFinePoint) -> Option<GlobalFinePoint> {
        let m = self.rows;
        let [x, y, z] = v.to_array();
        match (
            checked_i48f16_from_row(m[0], x, y, z),
            checked_i48f16_from_row(m[1], x, y, z),
            checked_i48f16_from_row(m[2], x, y, z),
        ) {
            (Some(x), Some(y), Some(z)) => Some(GlobalFinePoint::new(x, y, z)),
            _ => None,
        }
    }

    /// The inverse rotation of a world-scale offset. The slow path's inverse.
    #[must_use]
    #[inline]
    pub const fn unrotate_global_fine(self, v: GlobalFinePoint) -> GlobalFinePoint {
        self.inverse().rotate_global_fine(v)
    }

    /// The inverse rotation of a world-scale offset, or `None` if out of range.
    #[must_use]
    #[inline]
    pub const fn checked_unrotate_global_fine(self, v: GlobalFinePoint) -> Option<GlobalFinePoint> {
        self.inverse().checked_rotate_global_fine(v)
    }

    /// Rotates a unit direction.
    ///
    /// Each component rounds once and clamps into `Signed32`, which is **not**
    /// a renormalize: rotating a unit direction repeatedly walks its length
    /// off, about `3.4e-9` relative per step. Call
    /// [`normalize`](corvid_vector::Direction::normalize) when a direction is
    /// being iterated rather than rotated once.
    #[must_use]
    #[inline]
    pub const fn rotate_direction(self, d: Direction) -> Direction {
        let m = self.rows;
        let [x, y, z] = d.to_array();
        Direction::new(
            signed_from_row(m[0], x, y, z),
            signed_from_row(m[1], x, y, z),
            signed_from_row(m[2], x, y, z),
        )
    }

    /// The inverse rotation of a unit direction.
    #[must_use]
    #[inline]
    pub const fn unrotate_direction(self, d: Direction) -> Direction {
        self.inverse().rotate_direction(d)
    }
}

/// Generates one row-times-vector reduction.
///
/// Every one of these is the same three statements -- three `to_bits` products
/// summed at `$acc`, one `round_shift` back from Q30, and one narrowing -- so
/// the rounding rule and the accumulation order live in exactly one place. The
/// parameters carry the only real differences: the accumulator width, and
/// whether leaving the output type's range saturates or reports.
macro_rules! define_row_fn {
    ($(#[$attr:meta])* $name:ident, $scalar:ident, $acc:ty, $round:ident, $repr:ty, saturating) => {
        $(#[$attr])*
        #[inline]
        const fn $name(row: [I2F30; 3], x: $scalar, y: $scalar, z: $scalar) -> $scalar {
            let rounded = $round(row_sum!($acc, row, x, y, z), 30);
            if rounded > $scalar::MAX.to_bits() as $acc {
                $scalar::MAX
            } else if rounded < $scalar::MIN.to_bits() as $acc {
                $scalar::MIN
            } else {
                $scalar::from_bits(rounded as $repr)
            }
        }
    };
    ($(#[$attr:meta])* $name:ident, $scalar:ident, $acc:ty, $round:ident, $repr:ty, checked) => {
        $(#[$attr])*
        #[inline]
        const fn $name(row: [I2F30; 3], x: $scalar, y: $scalar, z: $scalar) -> Option<$scalar> {
            let rounded = $round(row_sum!($acc, row, x, y, z), 30);
            if rounded > $scalar::MAX.to_bits() as $acc || rounded < $scalar::MIN.to_bits() as $acc {
                None
            } else {
                Some($scalar::from_bits(rounded as $repr))
            }
        }
    };
}

/// `row * (x, y, z)` at Q60, accumulated at `$acc`.
macro_rules! row_sum {
    ($acc:ty, $row:expr, $x:expr, $y:expr, $z:expr) => {
        ($row[0].to_bits() as $acc) * ($x.to_bits() as $acc)
            + ($row[1].to_bits() as $acc) * ($y.to_bits() as $acc)
            + ($row[2].to_bits() as $acc) * ($z.to_bits() as $acc)
    };
}

define_row_fn! {
    /// One basis row against three `I16F16` components.
    i16f16_from_row, I16F16, i64, round_shift_i64, i32, saturating
}

define_row_fn! {
    /// One basis row against three `I16F16` components, or `None` if out of range.
    checked_i16f16_from_row, I16F16, i64, round_shift_i64, i32, checked
}

define_row_fn! {
    /// One basis row against three `I24F8` components.
    i24f8_from_row, I24F8, i64, round_shift_i64, i32, saturating
}

define_row_fn! {
    /// One basis row against three `I24F8` components, or `None` if out of range.
    checked_i24f8_from_row, I24F8, i64, round_shift_i64, i32, checked
}

define_row_fn! {
    /// One basis row against three `I48F16` components. The `i128` slow path.
    i48f16_from_row, I48F16, i128, round_shift_i128, i64, saturating
}

define_row_fn! {
    /// One basis row against three `I48F16` components, or `None` if out of range.
    checked_i48f16_from_row, I48F16, i128, round_shift_i128, i64, checked
}

/// One basis row against three `Signed32` components.
///
/// Not `define_row_fn!` for two reasons: `Signed32::MIN` is `-MAX` by the SNORM
/// convention, so the clamp is symmetric rather than the fixed-point family's
/// asymmetric one; and the operands are canonicalized first, because `Signed32`
/// spends `i32::MIN` and `-(2^31 - 1)` on the same `-1.0` and two directions
/// that compare and hash equal must not rotate to different ones.
#[inline]
const fn signed_from_row(row: [I2F30; 3], x: Signed32, y: Signed32, z: Signed32) -> Signed32 {
    let (x, y, z) = (x.canonicalize(), y.canonicalize(), z.canonicalize());
    let rounded = round_shift_i64(row_sum!(i64, row, x, y, z), 30);
    let limit = Signed32::MAX.to_bits() as i64;
    if rounded > limit {
        Signed32::MAX
    } else if rounded < -limit {
        Signed32::MIN
    } else {
        Signed32::from_bits(rounded as i32)
    }
}

impl core::fmt::Debug for Basis {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Basis[right {:?}, forward {:?}, up {:?}]",
            self.right(),
            self.forward(),
            self.up()
        )
    }
}

/// One basis row against three `I16F16` components, widened into `I48F16`.
///
/// The accumulation is the same `i32 x i32 -> i64` the near-field rotation uses;
/// only the output type is wider, which is what removes the saturation without
/// touching the arithmetic.
#[inline]
const fn i48f16_from_fine_row(row: [I2F30; 3], x: I16F16, y: I16F16, z: I16F16) -> I48F16 {
    let sum = (row[0].to_bits() as i64) * (x.to_bits() as i64)
        + (row[1].to_bits() as i64) * (y.to_bits() as i64)
        + (row[2].to_bits() as i64) * (z.to_bits() as i64);
    I48F16::from_bits(round_shift_i64(sum, 30))
}
