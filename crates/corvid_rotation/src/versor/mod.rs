//! [`Versor`]: a rotation as a unit quaternion of [`I2F30`] components.

use corvid_fixed::I2F30;

use crate::basis::Basis;

/// `1.0` at the Q30 scale the components use.
const ONE: i64 = 1 << 30;

/// How far from unit-norm [`Versor::from_xyzw`] will still accept, in Q30 last
/// bits.
///
/// The same figure as `Basis`'s `ORTHONORMAL_TOLERANCE` and for the same
/// reason -- room for a rotation that arrived over a wire as `f32` -- but **not
/// the same window**: a basis entry is quadratic in these components, so the
/// two gates disagree on inputs from about half the tolerance up, and a versor
/// this accepts can produce a basis `Basis::from_rows` rejects.
const UNIT_TOLERANCE: i64 = 1 << 14;

/// How close two versors must be, in Q30 last bits of their dot product,
/// before [`Versor::slerp`] hands over to [`Versor::nlerp`].
///
/// `1 << 12` is a dot product of `1 - 3.8e-6`, which is `0.316 deg` apart. Below
/// that the `sin(theta)` the slerp weights divide by has lost most of its
/// significant bits, and the two interpolations agree to well under a last bit
/// anyway.
const SLERP_FALLBACK: i64 = 1 << 12;

/// A rotation as a unit quaternion of four [`I2F30`] components: 16 bytes.
///
/// Composing is 16 multiplies against a [`Basis`]'s 27, at 44% of the size --
/// measured at 17.6 ns against the matrix's 35.6 ns. Rotating a *point* goes
/// through the matrix form and so costs strictly more than using a [`Basis`]
/// directly: 38.5 ns against 12.6 ns. Compose as a versor, rotate as a basis.
/// (`examples/rotation_bench.rs`; the figures move with the host, the ordering
/// does not.)
///
/// Repeated composition needs [`renormalize`](Self::renormalize), which the
/// matrix does not.
/// Anything long-lived should round-trip through [`Rotation`](crate::Rotation)
/// or [`FineRotation`](crate::FineRotation): re-encoding lands on the same bits
/// every time, so a packed rotation cannot drift however often it is decoded
/// and packed again.
///
/// Components are stored in `x`, `y`, `z`, `w` order.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct Versor {
    components: [I2F30; 4],
}

impl Default for Versor {
    #[inline]
    fn default() -> Self {
        Self::IDENTITY
    }
}
mod algebra;
mod apply;

impl From<Versor> for Basis {
    #[inline]
    fn from(q: Versor) -> Self {
        q.to_basis()
    }
}

impl From<Basis> for Versor {
    #[inline]
    fn from(m: Basis) -> Self {
        Self::from_basis(m)
    }
}

impl core::fmt::Debug for Versor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let q = self.components;
        write!(
            f,
            "Versor({}, {}, {}, {})",
            q[0].to_f64(),
            q[1].to_f64(),
            q[2].to_f64(),
            q[3].to_f64()
        )
    }
}
