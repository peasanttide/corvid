//! How a world-scale point comes back into each tier.
//!
//! The seam against `transform.rs` is direction. Every operation there widens
//! to [`GlobalFinePoint`] and works in one type, which is what lets one macro
//! body serve all three tiers; what cannot be shared is the way back, because
//! each tier's position type has its own range and its own answer for a point
//! outside it. That answer is always to clamp rather than to refuse: a
//! transform has nowhere to put a refusal.

use corvid_fixed::{I24F8, I48F16};
use corvid_vector::{FinePoint, GlobalFinePoint, GlobalPoint};

use crate::{FineTransform, StageTransform, Transform};

impl Transform {
    /// Narrows a world-scale point back into this tier's position type,
    /// saturating.
    #[inline]
    pub(crate) const fn narrow_saturating(point: GlobalFinePoint) -> GlobalPoint {
        if let Some(narrowed) = point.to_global() {
            narrowed
        } else {
            // Past `GlobalPoint`'s range on at least one axis. Clamp each axis
            // independently, which is what the point type's own saturating
            // arithmetic would do.
            let [x, y, z] = point.to_array();
            GlobalPoint::new(saturate_global(x), saturate_global(y), saturate_global(z))
        }
    }
}

impl FineTransform {
    /// The identity narrowing: this tier already works at `I48F16`.
    #[inline]
    pub(crate) const fn narrow_saturating(point: GlobalFinePoint) -> GlobalFinePoint {
        point
    }
}

impl StageTransform {
    /// The same pose in the world tier, exactly.
    ///
    /// Both tiers carry sixteen fractional bits and the same packed rotation,
    /// so nothing rounds and nothing can fail. It is the direction a stage pose
    /// travels: out of the room and into the world.
    #[must_use]
    #[inline]
    pub const fn to_fine(self) -> FineTransform {
        FineTransform::new(self.origin(), self.rotation())
    }

    /// Narrows a world-scale point back into this tier's position type,
    /// saturating.
    #[inline]
    pub(crate) const fn narrow_saturating(point: GlobalFinePoint) -> FinePoint {
        point.to_fine_saturating()
    }
}

/// Clamps one `I48F16` component into `I24F8`.
#[inline]
const fn saturate_global(value: I48F16) -> I24F8 {
    let bits = value.to_bits();
    // Round the eight fractional bits away first, then clamp. The rounding runs
    // on the unsigned magnitude: this is reached precisely when a component has
    // saturated, so `bits` can be `i64::MAX` and `bits + half` would overflow.
    let scaled = ((bits.unsigned_abs() + (1u64 << 7)) >> 8) as i64;
    let scaled = if bits >= 0 { scaled } else { -scaled };
    if scaled > I24F8::MAX.to_bits() as i64 {
        I24F8::MAX
    } else if scaled < I24F8::MIN.to_bits() as i64 {
        I24F8::MIN
    } else {
        I24F8::from_bits(scaled as i32)
    }
}
