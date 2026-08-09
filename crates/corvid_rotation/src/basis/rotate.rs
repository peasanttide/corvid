//! Applying a basis to a vector, at each of the widths a position comes in.
//!
//! Every row here is the same dot product at a different accumulator width, so
//! the six of them are generated rather than written out: `define_row_fn!`
//! declares one, and `row_sum!` is the accumulation each declares.

use corvid_fixed::{I2F30, I16F16, I24F8, I48F16, Signed32};
use corvid_vector::{Direction, FinePoint, GlobalFinePoint, GlobalPoint};

use super::{Basis, round_shift_i64, round_shift_i128};

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
