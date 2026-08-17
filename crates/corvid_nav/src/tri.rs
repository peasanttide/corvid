//! One triangle of the surface: its vertices, its frame and its limits.

use corvid_fixed::{I16F16, I24F8, Signed32};
use corvid_vector::{Direction, FinePoint, GlobalPoint};

use crate::error::NavError;
use crate::linear::{Linear3, cross_bits};
use crate::seam::NavTriEdge;

/// The longest edge a face may have.
///
/// A local coordinate is one byte across a whole triangle, so the edge length
/// is what its resolution is measured against: eight metres over 255 codes is
/// 3.1 cm. A longer edge would coarsen every position in the level without
/// saying so, which is why [`NavMesh::new`](crate::NavMesh::new) refuses one.
pub const MAX_EDGE: I24F8 = I24F8::from_bits(8 << 8);

/// The shallowest a face's normal may lean before its local frame gives out.
///
/// Height is measured along the geocentric up, so a face's local frame is
/// singular exactly when its plane contains that axis -- a wall. This is
/// `cos(80 degrees)`, evaluated at compile time, and it is the *only* slope
/// limit the representation itself imposes; a game's idea of "too steep to
/// walk" is a [`Tune`](crate::Tune) field and is looser than this by design.
const MIN_SLOPE_COSINE: Signed32 = Signed32::from_f64(0.173_648_177_666_930_35);

/// One triangle of the surface.
///
/// Its three vertices are ECEF, its local frame is barycentric in the first two
/// axes and metres of height along the geocentric up in the third, and the two
/// matrices between them are stored rather than derived because every position
/// in the level goes through one of them.
///
/// The local frame's origin is vertex 2, and its columns are `a - c`, `b - c`
/// and the up direction, which is exactly the affine combination [crs.md]
/// writes:
///
/// ```text
/// g = x*a + y*b + (1 - x - y)*c + z*normalize(a + b + c)
/// ```
///
/// The plane of the triangle is therefore `z == 0` in local coordinates, which
/// is what makes [`calc_collision_vs_plane`](crate::calc_collision_vs_plane) a
/// sign test on one number.
///
/// [crs.md]: https://github.com/peasanttide/peasanttide/blob/main/design/crs.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NavTri {
    triangle: [GlobalPoint; 3],
    edges: [Option<NavTriEdge>; 3],
    down: Direction,
    normal: Direction,
    local_to_ecef: Linear3,
    ecef_to_local: Linear3,
}

impl NavTri {
    /// The three ECEF vertices.
    #[must_use]
    #[inline]
    pub const fn triangle(&self) -> [GlobalPoint; 3] {
        self.triangle
    }

    /// The three seams, in edge order.
    #[must_use]
    #[inline]
    pub const fn edges(&self) -> [Option<NavTriEdge>; 3] {
        self.edges
    }

    /// One seam, or [`None`] both when the edge is a boundary and when the
    /// index is not an edge.
    #[must_use]
    #[inline]
    pub const fn edge(&self, index: usize) -> Option<NavTriEdge> {
        match index {
            0 => self.edges[0],
            1 => self.edges[1],
            2 => self.edges[2],
            _ => None,
        }
    }

    /// Which way gravity pulls here: away from the local up.
    ///
    /// In local coordinates this is exactly `-Z`, because the height axis *is*
    /// the up axis. That is the whole reason the frame is built the way it is:
    /// gravity acts on one component and a ballistic substep needs no rotation.
    #[must_use]
    #[inline]
    pub const fn down(&self) -> Direction {
        self.down
    }

    /// The face normal, oriented away from the earth's centre.
    ///
    /// Independent of the winding, so a mesh whose faces disagree about which
    /// way round they are still collides correctly. What the winding does
    /// decide is the sign of the local frame's determinant, which nothing
    /// downstream reads.
    #[must_use]
    #[inline]
    pub const fn normal(&self) -> Direction {
        self.normal
    }

    /// The map from local coordinates to an ECEF offset from vertex 2.
    #[must_use]
    #[inline]
    pub const fn local_to_ecef(&self) -> Linear3 {
        self.local_to_ecef
    }

    /// The map from an ECEF offset from vertex 2 to local coordinates.
    #[must_use]
    #[inline]
    pub const fn ecef_to_local(&self) -> Linear3 {
        self.ecef_to_local
    }

    /// Vertex 2, which is where the local frame is anchored.
    #[must_use]
    #[inline]
    pub const fn origin(&self) -> GlobalPoint {
        self.triangle[2]
    }

    /// The ECEF position a local coordinate names.
    #[must_use]
    #[inline]
    pub const fn ecef(&self, local: FinePoint) -> GlobalPoint {
        let offset = self.local_to_ecef.apply(local);
        self.origin().add(GlobalPoint::new(
            offset.x().to_i24f8(),
            offset.y().to_i24f8(),
            offset.z().to_i24f8(),
        ))
    }

    /// The local coordinate an ECEF position sits at.
    ///
    /// The offset from the origin is taken in [`I24F8`] and narrowed, so a
    /// target further than 32.7 km away saturates and the answer only points
    /// the right way rather than naming the place. That is enough for
    /// [`NavMesh::walk_toward`](crate::NavMesh::walk_toward), which is the only
    /// caller that ever asks about somewhere it is not.
    #[must_use]
    #[inline]
    pub const fn local(&self, ecef: GlobalPoint) -> FinePoint {
        let offset = ecef.sub(self.origin());
        self.ecef_to_local.apply(FinePoint::new(
            offset.x().to_i16f16(),
            offset.y().to_i16f16(),
            offset.z().to_i16f16(),
        ))
    }

    /// Whether a local coordinate's barycentric part lies within the triangle.
    #[must_use]
    #[inline]
    pub const fn contains(local: FinePoint) -> bool {
        let x = local.x().to_bits();
        let y = local.y().to_bits();
        x >= 0 && y >= 0 && (x as i64) + (y as i64) <= I16F16::ONE.to_bits() as i64
    }

    /// The nearest local coordinate that is inside the triangle and not
    /// underground.
    ///
    /// Every event in a step ends with this, which is what makes "a `NavCords`
    /// is on the surface by construction" true of the arithmetic and not only
    /// of the encoding: rounding may put a crossing a last bit outside, and the
    /// answer is dragged back rather than allowed to name a point in another
    /// triangle.
    #[must_use]
    #[inline]
    pub const fn clamp_inside(local: FinePoint) -> FinePoint {
        let mut x = at_least_zero(local.x().to_bits()) as i64;
        let mut y = at_least_zero(local.y().to_bits()) as i64;
        let one = I16F16::ONE.to_bits() as i64;
        let over = x + y - one;
        if over > 0 {
            // Both weights give up the same share, so a point outside a corner
            // lands on the edge nearest it rather than on whichever axis the
            // code happened to test first.
            let half = over / 2;
            x -= half;
            y -= over - half;
            if y < 0 {
                x += y;
                y = 0;
            }
            if x < 0 {
                y += x;
                x = 0;
            }
        }
        FinePoint::new(
            I16F16::from_bits(x as i32),
            I16F16::from_bits(y as i32),
            I16F16::from_bits(at_least_zero(local.z().to_bits())),
        )
    }

    /// Builds a triangle's frame from its three ECEF vertices.
    ///
    /// The edges are filled in afterwards by
    /// [`NavMesh::new`](crate::NavMesh::new), which is the only thing that can
    /// know them.
    pub(crate) fn build(face: usize, triangle: [GlobalPoint; 3]) -> Result<Self, NavError> {
        let [a, b, c] = triangle;
        for (from, to) in [(a, b), (b, c), (c, a)] {
            let limit = MAX_EDGE.to_bits() as u64;
            match from.checked_sub(to) {
                Some(edge) if edge.length_squared() <= limit * limit => {}
                _ => return Err(NavError::EdgeTooLong { face }),
            }
        }

        // The up at the triangle is the direction of its centroid from the
        // earth's centre, and the centroid is three world positions summed:
        // past what a `GlobalPoint` holds, and exactly what a ratio is for.
        let up = Direction::from_ratio(centroid_ratio(triangle))
            .ok_or(NavError::DegenerateFace { face })?;

        let first = fine_offset(a, c);
        let second = fine_offset(b, c);
        let raw = Direction::from_ratio(narrow_ratio(cross_bits(first, second)))
            .ok_or(NavError::DegenerateFace { face })?;

        let cosine = raw.align(up);
        if cosine.abs() < MIN_SLOPE_COSINE {
            return Err(NavError::FaceTooSteep { face });
        }
        let normal = if cosine.is_negative() { raw.neg() } else { raw };

        let local_to_ecef = Linear3::from_columns([first, second, fine_direction(up)]);
        let ecef_to_local = local_to_ecef
            .inverse()
            .ok_or(NavError::DegenerateFace { face })?;

        Ok(Self {
            triangle,
            edges: [None; 3],
            down: up.neg(),
            normal,
            local_to_ecef,
            ecef_to_local,
        })
    }

    /// Records a seam. Called once per edge while the mesh is being built.
    pub(crate) const fn set_edge(&mut self, index: usize, edge: NavTriEdge) {
        if index < 3 {
            self.edges[index] = Some(edge);
        }
    }
}

/// `value`, or zero if it is below.
#[inline]
const fn at_least_zero(value: i32) -> i32 {
    if value < 0 { 0 } else { value }
}

/// The three vertices summed, at full width.
///
/// Three world positions do not fit a [`GlobalPoint`] -- three earth radii is
/// past the +/-8388 km it holds -- and only the ratio is wanted, so the sum stays
/// as bit patterns and never becomes a point at all.
#[inline]
const fn centroid_ratio(triangle: [GlobalPoint; 3]) -> [i64; 3] {
    let [a, b, c] = triangle;
    [
        a.x().to_bits() as i64 + b.x().to_bits() as i64 + c.x().to_bits() as i64,
        a.y().to_bits() as i64 + b.y().to_bits() as i64 + c.y().to_bits() as i64,
        a.z().to_bits() as i64 + b.z().to_bits() as i64 + c.z().to_bits() as i64,
    ]
}

/// `from - to` as a near-field offset. Both are vertices of one triangle, so
/// the difference is at most [`MAX_EDGE`] and the narrowing is exact.
#[inline]
pub(crate) const fn fine_offset(from: GlobalPoint, to: GlobalPoint) -> FinePoint {
    let offset = from.sub(to);
    FinePoint::new(
        offset.x().to_i16f16(),
        offset.y().to_i16f16(),
        offset.z().to_i16f16(),
    )
}

/// A unit direction as a near-field vector, which is what a matrix column is.
///
/// [`Signed32`] divides by `2^31 - 1` and [`I16F16`] by `2^16`, so this is the
/// one rescale in the crate that is not a shift.
#[inline]
pub(crate) const fn fine_direction(direction: Direction) -> FinePoint {
    let [x, y, z] = direction.to_array();
    FinePoint::new(fine_component(x), fine_component(y), fine_component(z))
}

/// One component of [`fine_direction`], rounded away from zero at the halfway
/// point so that a direction and its opposite stay exact negatives.
#[inline]
const fn fine_component(value: Signed32) -> I16F16 {
    let scale = Signed32::MAX.to_bits() as i64;
    let bits = value.canonicalize().to_bits() as i64;
    let scaled = bits << 16;
    let rounded = if scaled >= 0 {
        (scaled + scale / 2) / scale
    } else {
        -((-scaled + scale / 2) / scale)
    };
    I16F16::from_bits(rounded as i32)
}

/// A `Q32` cross product reduced to the width [`Direction::from_ratio`] takes.
///
/// Only the ratios matter, so a shift that brings the largest component inside
/// an `i64` changes the answer by less than a unit direction's own last bit.
#[inline]
const fn narrow_ratio(cross: [i128; 3]) -> [i64; 3] {
    let mut largest = cross[0].unsigned_abs();
    if cross[1].unsigned_abs() > largest {
        largest = cross[1].unsigned_abs();
    }
    if cross[2].unsigned_abs() > largest {
        largest = cross[2].unsigned_abs();
    }
    let bit_length = 128 - largest.leading_zeros();
    let down = bit_length.saturating_sub(62);
    [
        (cross[0] >> down) as i64,
        (cross[1] >> down) as i64,
        (cross[2] >> down) as i64,
    ]
}
