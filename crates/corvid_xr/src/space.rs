//! Where the stage is in the world, and how big a stage metre is.
//!
//! One [`Anchor`] carries both scales. A defender standing on the surface has
//! `metres == 1` and an origin on the cell under their feet; a swarm player
//! holding the planet at arm's length has the same type with `metres` in the
//! thousands. Diving from one to the other is a camera transition, so it costs
//! nothing, needs no agreement, and cannot desync -- which is why an `Anchor` is
//! the client's own and never reaches a simulation tick.

use corvid_fixed::{Factor16, Factor32, I16F16, I48F16};

use corvid_rotation::{FineRotation, Versor};
use corvid_transform::FineTransform;

use corvid_vector::GlobalFinePoint;
use serde::{Deserialize, Serialize};

use crate::Pose;

/// Which of a game's two views is showing.
///
/// A different word from [`Space`](crate::Space) on purpose: a `Space` names
/// the reference frame a runtime reports a pose in, and a `Scale` names how
/// much world one stage metre buys.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Scale {
    /// The planet held at arm's length, a metre or so across.
    ///
    /// A cell is microscopic here -- with a 2 856 m radius held as a 1 m model a
    /// stage millimetre is 5.712 m of world -- so **pointing at table scale is a
    /// raycast rather than a hand**. The ray leaves the controller, meets the
    /// planet in world space, and the hit is snapped to the nearest cell. The
    /// precision does not come from the hand.
    #[default]
    Table,
    /// Standing on the surface, at human scale, where one stage metre is one
    /// world metre.
    Surface,
}

impl Scale {
    /// Whether one stage metre is one world metre.
    #[must_use]
    #[inline]
    pub const fn is_human(self) -> bool {
        matches!(self, Self::Surface)
    }
}

/// Where in the world the stage is, and how big a stage metre is.
///
/// ```
/// use corvid_xr::{Anchor, Pose};
/// use corvid_fixed::I16F16;
/// use corvid_rotation::FineRotation;
/// use corvid_vector::{GlobalFinePoint, globalfinepoint};
///
/// // A defender on the surface: one stage metre is one world metre.
/// let standing = Anchor::standing(globalfinepoint(0, 0, 2_856), FineRotation::IDENTITY);
/// assert_eq!(standing.metres, I16F16::ONE);
///
/// // A swarm player holding the same planet -- 5 712 m across -- as a 1 m model.
/// let held = Anchor::holding(
///     GlobalFinePoint::ZERO,
///     I16F16::from_f64(5_712.0),
///     I16F16::ONE,
///     Pose::IDENTITY,
/// );
/// assert_eq!(held.metres, I16F16::from_f64(5_712.0));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Anchor {
    /// Where in the world the stage's origin sits.
    pub origin: GlobalFinePoint,
    /// How the stage is turned relative to the world.
    pub rotation: FineRotation,
    /// How many world metres one stage metre is. One at human scale.
    ///
    /// Zero is not a scale, and an anchor holding it converts nothing:
    /// [`to_world`](Self::to_world) collapses the stage onto the origin and
    /// [`to_stage`](Self::to_stage) saturates, which is what the fixed-point
    /// arithmetic under it does rather than a rule this type adds.
    pub metres: I16F16,
}

impl Default for Anchor {
    /// At the world's origin, unturned, at human scale.
    #[inline]
    fn default() -> Self {
        Self::standing(GlobalFinePoint::ZERO, FineRotation::IDENTITY)
    }
}

impl Anchor {
    /// Standing on the world at human scale, facing along the rotation.
    #[must_use]
    #[inline]
    pub const fn standing(origin: GlobalFinePoint, rotation: FineRotation) -> Self {
        Self {
            origin,
            rotation,
            metres: I16F16::ONE,
        }
    }

    /// Holding something `across` metres wide as a `held` metre-wide model,
    /// centred at the stage pose `ahead`.
    ///
    /// The stage is turned the way `ahead` faces, and the origin follows from
    /// the rest: whatever the rotation, [`to_stage`](Self::to_stage) of
    /// `centre` is `ahead`'s position.
    ///
    /// ```
    /// use corvid_xr::{Anchor, Pose};
    /// use corvid_fixed::I16F16;
    /// use corvid_rotation::FineRotation;
    /// use corvid_vector::{FinePoint, GlobalFinePoint};
    ///
    /// let reach = FinePoint::new(I16F16::ZERO, I16F16::from_f64(0.6), I16F16::from_f64(1.4));
    /// let ahead = Pose::new(reach, FineRotation::IDENTITY);
    /// let held = Anchor::holding(
    ///     GlobalFinePoint::ZERO,
    ///     I16F16::from_f64(5_712.0),
    ///     I16F16::ONE,
    ///     ahead,
    /// );
    /// assert_eq!(held.to_world(ahead).position(), GlobalFinePoint::ZERO);
    /// ```
    #[must_use]
    pub const fn holding(
        centre: GlobalFinePoint,
        across: I16F16,
        held: I16F16,
        ahead: Pose,
    ) -> Self {
        let rotation = ahead.rotation();
        let metres = across.saturating_div(held);
        let offset = rotation
            .to_basis()
            .rotate_global_fine(ahead.origin().mul(widen(metres)));
        Self {
            origin: centre.sub(offset),
            rotation,
            metres,
        }
    }

    /// The same anchor, turned. What a grab does to a held planet.
    #[must_use]
    #[inline]
    pub const fn with_rotation(self, rotation: FineRotation) -> Self {
        Self { rotation, ..self }
    }

    /// The same anchor at a different scale, keeping the origin and the facing.
    #[must_use]
    #[inline]
    pub const fn with_metres(self, metres: I16F16) -> Self {
        Self { metres, ..self }
    }

    /// Where a stage pose is, in the world.
    ///
    /// A [`FineTransform`] rather than a [`Pose`]: the answer is a world
    /// position, and the widening it goes through is exact.
    #[must_use]
    pub const fn to_world(self, pose: Pose) -> FineTransform {
        let scaled = pose.origin().mul(widen(self.metres));
        let turned = self.rotation.to_basis().rotate_global_fine(scaled);
        FineTransform::new(
            turned.add(self.origin),
            composed(self.rotation, pose.rotation()),
        )
    }

    /// Where a world position is, in the stage.
    ///
    /// The rotation of the answer is the identity: a position has no facing.
    /// [`to_stage_pose`](Self::to_stage_pose) is the one that carries one.
    #[must_use]
    pub const fn to_stage(self, at: GlobalFinePoint) -> Pose {
        let local = self
            .rotation
            .to_basis()
            .unrotate_global_fine(at.sub(self.origin));
        let scale = widen(self.metres);
        let [x, y, z] = local.to_array();
        // Saturating, because a stage pose has nowhere to put a refusal. It is
        // reached only by a world position more than 32 km of stage away, which
        // is a position outside the room the stage is.
        let scaled = GlobalFinePoint::new(
            x.saturating_div(scale),
            y.saturating_div(scale),
            z.saturating_div(scale),
        );
        Pose::new(scaled.to_fine_saturating(), FineRotation::IDENTITY)
    }

    /// Where a world pose is, in the stage: [`to_world`](Self::to_world)
    /// undone.
    #[must_use]
    pub const fn to_stage_pose(self, world: FineTransform) -> Pose {
        let undo = if is_identity(self.rotation) {
            FineRotation::IDENTITY
        } else {
            FineRotation::from_versor(self.rotation.to_versor().inverse())
        };
        self.to_stage(world.position())
            .with_rotation(composed(undo, world.rotation()))
    }

    /// Interpolate between two anchors, for the dive between scales.
    ///
    /// Exact at both ends, like every interpolation in this workspace: weight
    /// [`Factor16::MIN`] is `self` and [`Factor16::MAX`] is `to`.
    #[must_use]
    pub fn lerp(self, to: Self, weight: Factor16) -> Self {
        if weight == Factor16::MIN {
            return self;
        }
        if weight == Factor16::MAX {
            return to;
        }
        let weight = broaden(weight);
        Self {
            origin: self.origin.lerp(to.origin, weight),
            rotation: FineRotation::from_versor(Versor::slerp(
                self.rotation.to_versor(),
                to.rotation.to_versor(),
                weight,
            )),
            metres: self.metres.lerp(to.metres, weight),
        }
    }
}

/// Whether a packed rotation is the identity.
#[inline]
const fn is_identity(rotation: FineRotation) -> bool {
    rotation.to_bits() == FineRotation::IDENTITY.to_bits()
}

/// `outer` applied after `inner`, and exactly so when either is the identity.
///
/// A packed rotation is quantized, so decoding two of them to versors,
/// composing, and packing the product again lands a last bit away from where it
/// started -- an unturned anchor would hand back a pose whose facing had moved
/// 0.0033 deg. Taking the two identity cases by hand is what makes an unturned
/// stage transparent, which is a property a test can see and a still head can
/// feel.
#[inline]
const fn composed(outer: FineRotation, inner: FineRotation) -> FineRotation {
    if is_identity(outer) {
        inner
    } else if is_identity(inner) {
        outer
    } else {
        FineRotation::from_versor(outer.to_versor().compose(inner.to_versor()))
    }
}

/// An [`I16F16`] as the [`I48F16`] the position arithmetic works in.
///
/// Exact: the two share their sixteen fractional bits, so this is the bit
/// pattern itself.
#[inline]
const fn widen(value: I16F16) -> I48F16 {
    I48F16::from_bits(value.to_bits() as i64)
}

/// A [`Factor16`] as a [`Factor32`], exactly at both ends.
///
/// Both are UNORM, so the widening is the pattern repeated rather than a shift:
/// `0xFFFF` becomes `0xFFFF_FFFF` and stays "all of it", where a shift would
/// leave one short of it and make an interpolation inexact at the far end.
#[inline]
const fn broaden(weight: Factor16) -> Factor32 {
    Factor32::from_bits(weight.to_bits() as u32 * 0x0001_0001)
}
