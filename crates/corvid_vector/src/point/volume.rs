//! The signed volume three offsets span, and the barycentric tests it serves.
//!
//! Split from [`WideOffset`](super::WideOffset) because a file stays under 400
//! lines, and this is the seam that was already there: everything in that file
//! answers a fixed-point type, and nothing here can.

use corvid_fixed::I24F8;

use super::{Direction, WideOffset};

/// What a unit [`Signed32`](corvid_fixed::Signed32) is, widened.
const UNIT: i128 = i32::MAX as i128;

impl WideOffset {
    /// The signed volume of the box these three offsets span.
    ///
    /// `self . (b x c)`, the scalar triple product. Positive when the three are
    /// right-handed in the order given, zero when they lie in a plane.
    #[must_use]
    #[inline]
    pub const fn volume(self, b: Self, c: Self) -> Volume {
        Volume(triple(
            [self.0[0] as i128, self.0[1] as i128, self.0[2] as i128],
            [b.0[0] as i128, b.0[1] as i128, b.0[2] as i128],
            [c.0[0] as i128, c.0[1] as i128, c.0[2] as i128],
        ))
    }

    /// The same, with a direction in the middle: `self . (direction x c)`.
    ///
    /// One [`Volume`] scale further out than [`volume`](Self::volume), by the
    /// direction's unit -- so the two are comparable only through
    /// [`Volume::distance_over`], which is where that factor is paid back.
    #[must_use]
    #[inline]
    pub const fn volume_across(self, direction: Direction, c: Self) -> Volume {
        let [dx, dy, dz] = direction.to_array();
        Volume(triple(
            [self.0[0] as i128, self.0[1] as i128, self.0[2] as i128],
            [
                super::signed32_bits(dx) as i128,
                super::signed32_bits(dy) as i128,
                super::signed32_bits(dz) as i128,
            ],
            [c.0[0] as i128, c.0[1] as i128, c.0[2] as i128],
        ))
    }
}

/// The signed volume of a box three offsets span.
///
/// Opaque on purpose. A volume of offsets that may each be wider than a point
/// has no fixed-point type to be -- `1.4e14` metres cubed is past every scalar
/// here -- and none of the things it is *for* need one: a barycentric
/// coordinate is a ratio of two volumes, and a winding is a sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Volume(i128);

impl Volume {
    /// Three offsets that lie in a plane.
    pub const ZERO: Self = Self(0);

    /// Whether the three offsets lie in a plane, which for a triangle means it
    /// is degenerate and for a ray means it is parallel to the face.
    #[must_use]
    #[inline]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Two volumes added, for a barycentric coordinate that is the sum of two.
    #[must_use]
    #[inline]
    pub const fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }

    /// Whether this falls outside `0 ..= bound`, with `bound`'s sign folded in.
    ///
    /// The comparison a barycentric test is, written as a comparison rather
    /// than as a division against zero and one. A division that rounded would
    /// let a ray through the seam between two triangles that share an edge,
    /// which is a hole in a mesh that appears at one pixel and never
    /// reproduces.
    #[must_use]
    #[inline]
    pub const fn is_outside(self, bound: Self) -> bool {
        if bound.0 > 0 {
            self.0 < 0 || self.0 > bound.0
        } else {
            self.0 > 0 || self.0 < bound.0
        }
    }

    /// `self / bound` as a distance in metres, or [`None`] if it overflows.
    ///
    /// The two volumes are **not** at the same scale, and the difference is the
    /// whole reason this is one operation rather than a division: `self` comes
    /// from [`WideOffset::volume`] and `bound` from
    /// [`volume_across`](WideOffset::volume_across), so `bound` carries one
    /// factor of a direction's unit that `self` does not. Paying it back is
    /// what turns the ratio into a length.
    ///
    /// [`None`] when that payment overflows, which takes geometry spanning most
    /// of the world -- reported as a miss rather than as a wrapped distance.
    #[must_use]
    #[inline]
    pub const fn distance_over(self, bound: Self) -> Option<I24F8> {
        if bound.0 == 0 {
            return None;
        }
        let Some(scaled) = self.0.checked_mul(UNIT) else {
            return None;
        };
        let quotient = divide(scaled, bound.0);
        // Two narrowings rather than one, because the type's own saturating
        // narrow starts one width in from here. Both clamp the same way, so
        // what a caller sees is a distance at the edge of the range.
        let clamped = if quotient > i64::MAX as i128 {
            i64::MAX
        } else if quotient < i64::MIN as i128 {
            i64::MIN
        } else {
            quotient as i64
        };
        Some(I24F8::saturating_from_bits(clamped))
    }
}

/// `a . (b x c)`, at whatever scale the three arrive in.
///
/// The widest it reaches is three offsets at `2^32` apiece, which is `2^98` --
/// so this is `i128` for a reason no reordering removes, and it is confined to
/// [`Volume`], whose whole contract is that the number never leaves.
#[inline]
const fn triple(a: [i128; 3], b: [i128; 3], c: [i128; 3]) -> i128 {
    a[0] * (b[1] * c[2] - b[2] * c[1])
        + a[1] * (b[2] * c[0] - b[0] * c[2])
        + a[2] * (b[0] * c[1] - b[1] * c[0])
}

/// A quotient, rounded to nearest with halves away from zero.
///
/// The wide twin of the one [`WideOffset::project`] uses. Rust's
/// integer division truncates toward zero, which turns every sub-unit shortfall
/// into a whole step in the last place.
#[inline]
const fn divide(numerator: i128, denominator: i128) -> i128 {
    let half = (denominator.unsigned_abs() / 2).cast_signed();
    let bump = if numerator < 0 { -half } else { half };
    (numerator + bump) / denominator
}
