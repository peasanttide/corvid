//! The volume a camera sees, as four numbers and no discriminant.

use corvid_fixed::{Angle16, I16F16};

use corvid_vector::FinePoint;
/// The volume a camera sees: a truncated pyramid, or the box it becomes when
/// the sides stop converging.
///
/// **One type covers perspective and orthographic, and it needs no tag to say
/// which.** The half-height at a forward distance `d` is a straight line --
///
/// ```text
/// h(d) = base + slope * d
/// ```
///
/// -- and the two cases are the two ends of it. A perspective frustum has
/// `base == 0`: the sides meet at the eye, so the height is proportional to
/// the distance. An orthographic box has `slope == 0`: the sides are parallel,
/// so the height is the same everywhere. Neither is a special case of the
/// other and both are ordinary values of this one.
///
/// That is what lets it be [`Pod`](bytemuck::Pod). An `enum` with two variants
/// would need a discriminant and could not be, and a struct with a `kind: u32`
/// would carry a field that is meaningless in one of its own modes.
///
/// # Why the law rather than the two heights
///
/// Storing the half-height *at each clip plane* would describe the same volume
/// and read more geometrically. It is not what is stored, because recovering
/// `base` from it costs a subtraction and a division at `I16F16`'s resolution,
/// and the answer for a perspective frustum comes back as a small non-zero
/// number rather than as zero. Storing the law keeps both cases exact: a
/// perspective frustum has `base` exactly zero and an orthographic one has
/// `slope` exactly zero, whatever the clip distances are.
///
/// [`near_half_height`](Self::near_half_height) and
/// [`far_half_height`](Self::far_half_height) are the geometric reading, and
/// they are derived.
///
/// # Vertical, and hor-plus
///
/// Every extent here is the **vertical** one. The horizontal follows from the
/// viewport's aspect ratio when the matrix is built, so a wider window sees
/// more sideways and the same amount up and down -- which is what a player on
/// an ultrawide monitor expects, and the opposite of what happens if the
/// stored extent is the horizontal one.
///
/// ```
/// use corvid_shape::Frustum;
/// use corvid_fixed::{Angle16, I16F16};
///
/// let lens = Frustum::perspective(
///     Angle16::from_degrees(90.0),
///     I16F16::from_f64(0.1),
///     I16F16::from_f64(100.0),
/// );
///
/// // Ninety degrees vertically is a slope of one: as high as it is far.
/// assert_eq!(lens.base, I16F16::ZERO);
/// assert!((lens.slope.to_f64() - 1.0).abs() < 1e-4);
///
/// let box_ = Frustum::orthographic(
///     I16F16::from_f64(10.0),
///     I16F16::from_f64(0.1),
///     I16F16::from_f64(100.0),
/// );
///
/// // A box does not widen with distance.
/// assert_eq!(box_.slope, I16F16::ZERO);
/// assert_eq!(box_.near_half_height(), box_.far_half_height());
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct Frustum {
    /// The near clip distance, in metres.
    pub near: I16F16,
    /// The far clip distance, in metres.
    pub far: I16F16,
    /// The half-height at the eye itself: zero for a perspective frustum.
    ///
    /// For an orthographic box this is half the viewport's height, and it is
    /// the whole of the height, since [`slope`](Self::slope) is then zero.
    pub base: I16F16,
    /// How fast the half-height grows per metre of distance: zero for an
    /// orthographic box.
    ///
    /// For a perspective frustum this is the tangent of half the vertical
    /// field of view.
    pub slope: I16F16,
}

impl Frustum {
    /// Sixty degrees: the vertical field of view a monitor at a desk wants.
    pub const DEFAULT_FOV: Angle16 = Angle16::from_degrees(60.0);

    /// Ten centimetres.
    pub const DEFAULT_NEAR: I16F16 = I16F16::from_f64(0.1);

    /// A kilometre.
    pub const DEFAULT_FAR: I16F16 = I16F16::from_f64(1000.0);

    /// Ten centimetres to a kilometre, sixty degrees up and down.
    ///
    /// The same value [`Default`] gives, as a `const` -- which is what lets a
    /// camera's own `new` stay a `const fn`. It can be one because the
    /// workspace's trigonometry is integer arithmetic all the way down, so the
    /// tangent behind it evaluates at compile time.
    pub const DEFAULT: Self =
        Self::perspective(Self::DEFAULT_FOV, Self::DEFAULT_NEAR, Self::DEFAULT_FAR);

    /// A perspective frustum: sides that meet at the eye.
    ///
    /// `fov_y` is the *vertical* field of view. The slope is its half-angle's
    /// tangent, computed from the workspace's integer sine and cosine rather
    /// than from a `tan` -- so a field of view of half a turn, whose tangent is
    /// infinite, saturates at [`I16F16::MAX`] instead of dividing by zero.
    /// That is a frustum nobody wants and it is not a panic.
    #[must_use]
    pub const fn perspective(fov_y: Angle16, near: I16F16, far: I16F16) -> Self {
        Self {
            near,
            far,
            base: I16F16::ZERO,
            slope: slope_of(fov_y),
        }
    }

    /// An orthographic box: sides that stay parallel.
    ///
    /// `height` is what the viewport spans vertically, in metres.
    #[must_use]
    #[inline]
    pub const fn orthographic(height: I16F16, near: I16F16, far: I16F16) -> Self {
        Self {
            near,
            far,
            base: I16F16::from_bits(height.to_bits() / 2),
            slope: I16F16::ZERO,
        }
    }

    /// The same frustum at a different vertical field of view.
    ///
    /// This makes it a perspective frustum whatever it was: a field of view is
    /// what a perspective frustum has, so setting one clears
    /// [`base`](Self::base).
    #[must_use]
    #[inline]
    pub const fn with_fov(self, fov_y: Angle16) -> Self {
        Self {
            near: self.near,
            far: self.far,
            base: I16F16::ZERO,
            slope: slope_of(fov_y),
        }
    }

    /// The half-height at a forward distance of `d` metres: `base + slope * d`.
    ///
    /// The one question this type answers, and the one every other method here
    /// is written in terms of.
    #[must_use]
    #[inline]
    pub const fn half_height_at(self, d: I16F16) -> I16F16 {
        self.base.saturating_add(self.slope.saturating_mul(d))
    }

    /// The half-height at the near clip plane.
    #[must_use]
    #[inline]
    pub const fn near_half_height(self) -> I16F16 {
        self.half_height_at(self.near)
    }

    /// The half-height at the far clip plane.
    #[must_use]
    #[inline]
    pub const fn far_half_height(self) -> I16F16 {
        self.half_height_at(self.far)
    }

    /// Whether the sides are parallel -- an orthographic box rather than a
    /// perspective frustum.
    ///
    /// A question asked of the data rather than a tag stored beside it, which
    /// is the whole point of the representation: there is no third answer and
    /// nothing that can disagree with the numbers.
    #[must_use]
    #[inline]
    pub const fn is_orthographic(self) -> bool {
        self.slope.to_bits() == 0
    }

    /// The vertical field of view, for a perspective frustum.
    ///
    /// The inverse of what [`perspective`](Self::perspective) was given, to
    /// within the resolution of an [`Angle16`]. An orthographic box has no
    /// field of view and answers zero.
    #[must_use]
    pub fn fov_y(self) -> Angle16 {
        if self.is_orthographic() {
            return Angle16::ZERO;
        }
        // `atan2` of the slope against one, doubled: the half-angle back to
        // the whole. Both arguments are Q16 so the ratio is the slope.
        let half = Angle16::atan2(
            i64::from(self.slope.to_bits()),
            i64::from(I16F16::ONE.to_bits()),
        );
        Angle16::from_bits(half.to_bits().wrapping_mul(2))
    }

    /// Whether a point in **eye space** is inside this volume.
    ///
    /// Eye space is the workspace's camera frame: `x` right, `y` forward, `z`
    /// up, with the eye at the origin. `aspect` is the viewport's width over
    /// its height, which is what turns the stored vertical extent into the
    /// horizontal one.
    ///
    /// This is the near half of frustum culling. A caller with a world
    /// position takes the eye out of it first, which is the same subtraction a
    /// camera does before anything reaches an `f32`.
    ///
    /// ```
    /// use corvid_shape::Frustum;
    /// use corvid_fixed::{Angle16, I16F16};
    /// use corvid_vector::FinePoint;
    ///
    /// let lens = Frustum::perspective(
    ///     Angle16::from_degrees(90.0),
    ///     I16F16::from_f64(1.0),
    ///     I16F16::from_f64(100.0),
    /// );
    /// let square = I16F16::ONE;
    ///
    /// // Ten metres ahead, on the axis.
    /// let ahead = FinePoint::from_array([I16F16::ZERO, I16F16::from_f64(10.0), I16F16::ZERO]);
    /// assert!(lens.contains(ahead, square));
    ///
    /// // Behind the near plane.
    /// let behind = FinePoint::from_array([I16F16::ZERO, I16F16::from_f64(-1.0), I16F16::ZERO]);
    /// assert!(!lens.contains(behind, square));
    /// ```
    #[must_use]
    pub fn contains(self, point: FinePoint, aspect: I16F16) -> bool {
        let [x, forward, z] = point.to_array();
        if forward < self.near || forward > self.far {
            return false;
        }
        let half = self.half_height_at(forward);
        let wide = half.saturating_mul(aspect);
        z.abs() <= half && x.abs() <= wide
    }

    /// Whether a sphere in **eye space** touches this volume.
    ///
    /// Conservative: it may answer `true` for a sphere just outside a corner,
    /// which for culling means drawing something that turns out to be
    /// invisible rather than omitting something that is not. It never answers
    /// `false` for a sphere that is partly inside.
    ///
    /// `aspect` means what it does in [`contains`](Self::contains).
    #[must_use]
    pub fn intersects_sphere(self, centre: FinePoint, radius: I16F16, aspect: I16F16) -> bool {
        let [x, forward, z] = centre.to_array();
        if forward < self.near.saturating_sub(radius) || forward > self.far.saturating_add(radius) {
            return false;
        }
        // The half-extents are taken at the sphere's own depth and widened by
        // its radius. That is the conservative test: a sphere whose centre is
        // outside the side planes but within a radius of them is kept.
        let clamped = if forward < self.near {
            self.near
        } else {
            forward
        };
        let half = self.half_height_at(clamped).saturating_add(radius);
        let wide = self
            .half_height_at(clamped)
            .saturating_mul(aspect)
            .saturating_add(radius);
        z.abs() <= half && x.abs() <= wide
    }
}

/// Ten centimetres to a kilometre, sixty degrees up and down.
///
/// A view is built by `Default` before the first frame, so a frustum whose
/// default is not a sensible one is a first frame drawn through whatever
/// `Default` put there -- and that first frame is the one a screenshot test
/// takes.
impl Default for Frustum {
    #[inline]
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The tangent of half an angle, in integers.
///
/// The one trigonometric quantity a frustum has. Computed from the workspace's
/// integer sine and cosine and divided in Q16, so it is bit-identical on every
/// target -- and so that a half-angle of a quarter turn, whose tangent is
/// infinite, saturates instead of dividing by zero.
const fn slope_of(fov_y: Angle16) -> I16F16 {
    let (sine, cosine) = fov_y.half().sin_cos();
    // `as` rather than `i64::from`, which is not callable in a `const fn`:
    // `From` is not a const trait yet. Both sources are `i32`, so the widening
    // is exact either way.
    let (sine, cosine) = (sine.to_bits() as i64, cosine.to_bits() as i64);
    if cosine == 0 {
        return I16F16::MAX;
    }
    // Q31 shifted into Q47 over Q31 is a Q16.
    I16F16::from_bits(corvid_bits::narrow_i64((sine << 16) / cosine))
}
