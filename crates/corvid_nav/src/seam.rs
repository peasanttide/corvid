//! The crossing between one triangle and the next.

use crate::cords::NavTriRef;
use crate::linear::{Affine3, Linear3};

/// The seam between two triangles.
///
/// Edge `i` runs between vertex `i` and vertex `i + 1 mod 3`, so edge 0 is the
/// line where the third barycentric weight is zero, edge 1 is where the first
/// is, and edge 2 is where the second is.
///
/// The transforms are what make crossing a seam a multiply rather than a
/// search: a position goes through [`local_to_next`](Self::local_to_next) and a
/// velocity through [`vel_to_next`](Self::vel_to_next), and nothing has to ask
/// the mesh where it now is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NavTriEdge {
    next: NavTriRef,
    walkable: bool,
    local_to_next: Affine3,
}

impl NavTriEdge {
    /// Records a seam. Only a mesh knows enough to build one.
    #[inline]
    pub(crate) const fn build(next: NavTriRef, walkable: bool, local_to_next: Affine3) -> Self {
        Self {
            next,
            walkable,
            local_to_next,
        }
    }

    /// The triangle on the other side.
    #[must_use]
    #[inline]
    pub const fn next(self) -> NavTriRef {
        self.next
    }

    /// Whether a body may cross this seam on foot.
    ///
    /// Precomputed by [`NavMesh::new`](crate::NavMesh::new) and derivable at
    /// any time from [`NavMesh::derive_walkable`](crate::NavMesh::derive_walkable),
    /// which is what `tests/walkable.rs` holds it to.
    #[must_use]
    #[inline]
    pub const fn is_walkable(self) -> bool {
        self.walkable
    }

    /// The map from this triangle's local frame into the neighbour's.
    #[must_use]
    #[inline]
    pub const fn local_to_next(self) -> Affine3 {
        self.local_to_next
    }

    /// The map that carries a velocity across, which is
    /// [`local_to_next`](Self::local_to_next)'s linear part.
    ///
    /// An accessor rather than a second field: the two matrices are the same
    /// matrix, and a stored copy of one of them is one more thing to keep level
    /// with the other.
    #[must_use]
    #[inline]
    pub const fn vel_to_next(self) -> Linear3 {
        self.local_to_next.linear()
    }
}
