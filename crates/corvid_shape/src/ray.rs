//! A ray, what it finds, and the trait that says a shape can be found.

use corvid_fixed::I24F8;

use crate::project::along;
use corvid_vector::{Direction, GlobalPoint};

/// A half-line: an origin and a unit direction.
///
/// The thing a cursor is. `Controller::look` builds one from the camera and the
/// pointer, casts it at whatever the game can be pointed at, and puts the
/// answer in its `View` — all of which is client-ring, none of which is hashed,
/// and all of which happens on the display's frame rather than the
/// simulation's, because that is what "feel is local" means.
///
/// The origin is a [`GlobalPoint`] — **world space**, not an offset from the
/// eye. That is this crate's scale throughout: a shape is an object, and an
/// object is somewhere in the world rather than somewhere relative to whoever
/// happens to be looking at it.
///
/// `I24F8` reaches ±8388 km at 3.9 mm an axis, which holds a planet ten
/// thousand kilometres across at a resolution far finer than a cursor can
/// distinguish. It is deliberately *not* `GlobalFinePoint`: that type's
/// 1.4e14 m is for a camera pose, where the eye's own position has to survive
/// being subtracted from — and paying its width in every shape a broad phase
/// walks would double the size of every bound for range nothing casts across.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct Ray {
    /// Where it starts.
    pub origin: GlobalPoint,
    /// Which way it goes. Unit, by the type.
    pub direction: Direction,
}

impl Ray {
    /// A ray from an origin along a direction.
    #[must_use]
    #[inline]
    pub const fn new(origin: GlobalPoint, direction: Direction) -> Self {
        Self { origin, direction }
    }

    /// Where the ray is, `distance` along it.
    ///
    /// Exact at zero: walking nowhere is where it started, bit for bit.
    ///
    /// ```
    /// use corvid_shape::Ray;
    /// use corvid_fixed::I24F8;
    /// use corvid_vector::{Direction, GlobalPoint, globalpoint};
    ///
    /// let ray = Ray::new(GlobalPoint::ZERO, Direction::Y);
    /// assert_eq!(ray.at(I24F8::from_f64(3.0)), globalpoint(0, 3, 0));
    /// assert_eq!(ray.at(I24F8::ZERO), GlobalPoint::ZERO);
    /// ```
    #[must_use]
    #[inline]
    pub fn at(self, distance: I24F8) -> GlobalPoint {
        self.origin + along(self.direction, distance)
    }

    /// Whether this ray goes nowhere.
    ///
    /// [`Direction::ZERO`] is representable — every point type in
    /// `corvid_vector` has a `ZERO`, and a `normalize` that failed has to
    /// answer *something* — so a ray built from one is a legal value that
    /// denotes no half-line at all. Every [`Cast`] in this crate answers
    /// [`None`] for it, which is the contract that method documents.
    #[must_use]
    #[inline]
    pub const fn is_degenerate(self) -> bool {
        self.direction.is_zero()
    }

    /// Casts this ray at a shape.
    ///
    /// The same call as [`Cast::cast`] with the two swapped round, because a
    /// reader thinks *this ray hits that shape* rather than the other way
    /// about. Both are here; neither is a second implementation.
    #[must_use]
    #[inline]
    pub fn cast_against<S: Cast + ?Sized>(self, shape: &S) -> Option<Hit> {
        shape.cast(self)
    }
}

/// Where a ray met a shape.
///
/// The `point` is redundant with `distance` — it is [`Ray::at`] of it — and is
/// carried anyway because every caller wants it and recomputing it is the same
/// arithmetic the cast already did.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct Hit {
    /// How far along the ray, in metres. Never negative.
    pub distance: I24F8,
    /// Where.
    pub point: GlobalPoint,
    /// The surface normal there, turned to face the ray.
    ///
    /// **Facing the ray**, always — a hit on the inside of a sphere or the back
    /// of a triangle answers the flipped normal rather than the geometric one.
    /// That is what a cursor decal and a rim light both want, and a caller that
    /// needs the geometric normal has the shape in hand to ask it for one.
    pub normal: Direction,
}

impl Hit {
    /// A hit at a distance along a ray, with a normal turned to face it.
    #[must_use]
    #[inline]
    pub fn new(ray: Ray, distance: I24F8, normal: Direction) -> Self {
        Self {
            distance,
            point: ray.at(distance),
            normal: facing(normal, ray),
        }
    }
}

/// A normal turned to face the ray that found it.
#[must_use]
#[inline]
pub(crate) fn facing(normal: Direction, ray: Ray) -> Direction {
    if crate::align(normal, ray.direction).is_positive() {
        -normal
    } else {
        normal
    }
}

/// A shape a ray can be cast at.
///
/// One method, because that is the whole of what this crate does with a shape.
/// It is a trait rather than an enum so that a game can cast at its own
/// geometry — a Goldberg cell, a swept capsule, a height field — through the
/// same call, and so that [`Ray::cast_against`] works on it without this crate
/// knowing it exists.
pub trait Cast {
    /// The nearest intersection at or in front of the ray's origin, if there is
    /// one.
    ///
    /// **In front**, always: a shape entirely behind the origin is a miss
    /// rather than a negative distance. A quadratic solved without that check
    /// puts the build cursor behind the player, which is the bug this sentence
    /// exists to prevent.
    ///
    /// **A [degenerate](Ray::is_degenerate) ray is a miss**, whatever it was
    /// cast at. A ray that goes nowhere arrives nowhere, and the alternative is
    /// worse than it sounds: the slab test's sentinels read as a hit at the far
    /// edge of the world, and the sphere's quadratic reports a positive
    /// distance to the origin the ray never left. Both are answers a caller
    /// would place a cursor with.
    fn cast(&self, ray: Ray) -> Option<Hit>;
}
