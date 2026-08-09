//! [`Basis`]: a rotation as a 3x3 matrix of [`I2F30`] entries.

use corvid_fixed::{I2F30, Signed32};

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
mod algebra;
mod rotate;

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
