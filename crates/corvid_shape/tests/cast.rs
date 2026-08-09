//! One hit and one miss per primitive, and the edge cases that are the reason
//! each cast is written the way it is.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::{I24F8, Signed32};
use corvid_shape::{Plane, Ray, Sphere, Triangle, align, project};
use corvid_vector::{Direction, GlobalPoint, globalpoint};

/// A metre, spelled once.
const fn metres(count: f64) -> I24F8 {
    I24F8::from_f64(count)
}

// ---------------------------------------------------------------- the ray ---

/// A ray walked a distance arrives where the arithmetic says it should.
#[test]
fn a_ray_walks() {
    let ray = Ray::new(GlobalPoint::ZERO, Direction::Y);
    assert_eq!(ray.at(metres(3.0)), globalpoint(0, 3, 0));
}

/// Walking zero is where it started, exactly. Endpoints being exact is what
/// every interpolation in this workspace owes, and this is the smallest
/// instance of it.
#[test]
fn walking_nowhere_is_where_it_started() {
    let ray = Ray::new(globalpoint(1, 2, 3), Direction::Y);
    assert_eq!(ray.at(I24F8::ZERO), globalpoint(1, 2, 3));
}

/// Walking backwards goes backwards rather than saturating at the origin.
#[test]
fn a_ray_walks_backwards() {
    let ray = Ray::new(globalpoint(0, 5, 0), Direction::Y);
    assert_eq!(ray.at(metres(-2.0)), globalpoint(0, 3, 0));
}

/// `project` is a dot product: along gives the length, across gives zero,
/// against gives a negative.
#[test]
fn projection_is_signed() {
    assert_eq!(project(globalpoint(0, 4, 0), Direction::Y), metres(4.0));
    assert_eq!(project(globalpoint(4, 0, 0), Direction::Y), I24F8::ZERO);
    assert_eq!(project(globalpoint(0, -4, 0), Direction::Y), metres(-4.0));
}

/// The full range does not overflow. This is the case an `i64` accumulator gets
/// wrong: three components at the top of a `GlobalPoint`'s range sum past 2^63, so
/// a narrower accumulator answers a large negative -- a hit behind the eye.
#[test]
fn projection_survives_the_full_range() {
    let far = GlobalPoint::from_array([I24F8::MAX; 3]);
    let diagonal = globalpoint(1, 1, 1)
        .normalize()
        .expect("it is not the origin");
    assert!(project(far, diagonal) > I24F8::ZERO);
}

/// `align` is the same product between two directions, and saturates to the
/// unit at either end rather than wrapping.
#[test]
fn alignment_is_a_cosine() {
    assert_eq!(align(Direction::Y, Direction::Y), Signed32::MAX);
    assert_eq!(align(Direction::Y, Direction::X), Signed32::ZERO);
    assert!(align(Direction::Y, -Direction::Y) < Signed32::ZERO);
}

// ------------------------------------------------------------- the sphere ---

fn ball() -> Sphere {
    Sphere::new(globalpoint(0, 10, 0), metres(2.0))
}

/// A ray fired at a sphere from outside hits its near face.
#[test]
fn a_ray_hits_a_sphere() {
    let hit = Ray::new(GlobalPoint::ZERO, Direction::Y)
        .cast_against(&ball())
        .expect("the ray points at it");
    assert_eq!(hit.distance, metres(8.0));
    assert_eq!(hit.point, globalpoint(0, 8, 0));
    assert_eq!(hit.normal, -Direction::Y);
}

/// A ray fired past it misses.
#[test]
fn a_ray_misses_a_sphere() {
    assert!(
        Ray::new(GlobalPoint::ZERO, Direction::X)
            .cast_against(&ball())
            .is_none()
    );
}

/// A sphere behind the ray is a miss and not a negative distance. This is what
/// a quadratic solved without the check gets wrong, and it is what puts the
/// build cursor behind the player.
#[test]
fn a_sphere_behind_the_ray_is_a_miss() {
    let behind = Sphere::new(globalpoint(0, -10, 0), metres(2.0));
    assert!(
        Ray::new(GlobalPoint::ZERO, Direction::Y)
            .cast_against(&behind)
            .is_none()
    );
}

/// A ray starting inside a sphere hits the far wall, from the inside -- and the
/// normal it reports faces the ray rather than outwards.
#[test]
fn a_ray_inside_a_sphere_hits_the_far_wall() {
    let around = Sphere::new(GlobalPoint::ZERO, metres(5.0));
    let hit = Ray::new(GlobalPoint::ZERO, Direction::Y)
        .cast_against(&around)
        .expect("it is surrounded");
    assert_eq!(hit.distance, metres(5.0));
    assert_eq!(hit.normal, -Direction::Y);
}

/// A tangent ray grazes rather than missing, which is the discriminant's zero.
#[test]
fn a_tangent_ray_grazes() {
    let beside = Sphere::new(globalpoint(2, 10, 0), metres(2.0));
    assert!(
        Ray::new(GlobalPoint::ZERO, Direction::Y)
            .cast_against(&beside)
            .is_some()
    );
}

/// Containment includes the boundary, and a negative radius holds nothing
/// rather than being an inside-out sphere.
#[test]
fn a_sphere_contains_its_own_skin() {
    let around = Sphere::new(GlobalPoint::ZERO, metres(5.0));
    assert!(around.contains(GlobalPoint::ZERO));
    assert!(around.contains(globalpoint(0, 5, 0)));
    assert!(!around.contains(globalpoint(0, 6, 0)));

    let nothing = Sphere::new(GlobalPoint::ZERO, metres(-1.0));
    assert!(!nothing.contains(GlobalPoint::ZERO));
    assert!(
        Ray::new(globalpoint(0, -5, 0), Direction::Y)
            .cast_against(&nothing)
            .is_none()
    );
}

// -------------------------------------------------------------- the plane ---

fn ground() -> Plane {
    Plane::through(GlobalPoint::ZERO, Direction::Z)
}

/// The ground plane, which is what a build cursor actually casts against.
#[test]
fn a_ray_hits_a_plane() {
    let hit = Ray::new(globalpoint(3, 4, 10), -Direction::Z)
        .cast_against(&ground())
        .expect("it points at the ground");
    assert_eq!(hit.distance, metres(10.0));
    assert_eq!(hit.point, globalpoint(3, 4, 0));
    assert_eq!(hit.normal, Direction::Z);
}

/// A ray parallel to a plane never reaches it, and does not divide by zero
/// finding that out.
#[test]
fn a_parallel_ray_misses_a_plane() {
    assert!(
        Ray::new(globalpoint(0, 0, 10), Direction::Y)
            .cast_against(&ground())
            .is_none()
    );
}

/// And one pointing away from it misses too.
#[test]
fn a_ray_pointing_away_misses_a_plane() {
    assert!(
        Ray::new(globalpoint(0, 0, 10), Direction::Z)
            .cast_against(&ground())
            .is_none()
    );
}

/// A plane not through the origin is where its offset says.
#[test]
fn a_plane_carries_its_offset() {
    let shelf = Plane::through(globalpoint(0, 0, 3), Direction::Z);
    assert_eq!(shelf.offset, metres(3.0));
    assert_eq!(shelf.distance_to(globalpoint(9, 9, 5)), metres(2.0));
    assert_eq!(shelf.distance_to(globalpoint(9, 9, 1)), metres(-2.0));

    let hit = Ray::new(globalpoint(0, 0, 10), -Direction::Z)
        .cast_against(&shelf)
        .expect("it points down");
    assert_eq!(hit.distance, metres(7.0));
}

// ----------------------------------------------------------- the triangle ---

/// Facing the origin, wound so its normal points back down the ray.
fn facing() -> Triangle {
    Triangle::new(
        globalpoint(-1, 5, -1),
        globalpoint(1, 5, -1),
        globalpoint(0, 5, 1),
    )
}

/// The centre of a triangle facing the ray.
#[test]
fn a_ray_hits_a_triangle() {
    let hit = Ray::new(GlobalPoint::ZERO, Direction::Y)
        .cast_against(&facing())
        .expect("it points at it");
    assert_eq!(hit.distance, metres(5.0));
    assert_eq!(hit.normal, -Direction::Y);
}

/// Just outside an edge is a miss. The barycentric test is where a naive
/// implementation lets a ray through the seam between two triangles that share
/// one.
#[test]
fn a_ray_past_the_edge_misses() {
    assert!(
        Ray::new(globalpoint(3, 0, 0), Direction::Y)
            .cast_against(&facing())
            .is_none()
    );
}

/// A triangle edge-on to the ray is a miss rather than a division by zero.
#[test]
fn an_edge_on_triangle_misses() {
    let edge_on = Triangle::new(
        globalpoint(-1, 0, 5),
        globalpoint(1, 0, 5),
        globalpoint(0, 0, 7),
    );
    assert!(
        Ray::new(GlobalPoint::ZERO, Direction::Y)
            .cast_against(&edge_on)
            .is_none()
    );
}

/// A back face is a hit, not a miss. A cursor cast at the inside of a planet's
/// shell is a legitimate hit and this crate does not decide otherwise.
#[test]
fn a_back_face_is_a_hit() {
    let away = Triangle::new(
        globalpoint(-1, 5, -1),
        globalpoint(0, 5, 1),
        globalpoint(1, 5, -1),
    );
    let hit = Ray::new(GlobalPoint::ZERO, Direction::Y)
        .cast_against(&away)
        .expect("a back face is still a face");
    assert_eq!(hit.distance, metres(5.0));
    // Still turned to face the ray, which is what makes the two windings
    // indistinguishable to a caller that only wanted to shade the hit.
    assert_eq!(hit.normal, -Direction::Y);
}

/// A triangle behind the ray is a miss.
#[test]
fn a_triangle_behind_the_ray_is_a_miss() {
    let behind = Triangle::new(
        globalpoint(-1, -5, -1),
        globalpoint(1, -5, -1),
        globalpoint(0, -5, 1),
    );
    assert!(
        Ray::new(GlobalPoint::ZERO, Direction::Y)
            .cast_against(&behind)
            .is_none()
    );
}

/// A degenerate triangle has no normal and cannot be hit.
#[test]
fn a_degenerate_triangle_has_no_normal() {
    let line = Triangle::new(
        GlobalPoint::ZERO,
        globalpoint(0, 5, 0),
        globalpoint(0, 10, 0),
    );
    assert!(line.normal().is_none());
    assert!(
        Ray::new(globalpoint(1, 0, 0), Direction::Y)
            .cast_against(&line)
            .is_none()
    );
}

/// A corner is inside rather than outside, so two triangles sharing one do not
/// both reject a ray through it.
#[test]
fn a_corner_is_a_hit() {
    let hit = Ray::new(globalpoint(0, 0, 1), Direction::Y).cast_against(&facing());
    assert!(hit.is_some());
}

/// The bounds hold all three corners and nothing more.
#[test]
fn a_triangle_bounds_itself() {
    let bounds = facing().bounds();
    assert_eq!(bounds.min, globalpoint(-1, 5, -1));
    assert_eq!(bounds.max, globalpoint(1, 5, 1));
}

/// A ray with no direction is not a ray, and cannot hit anything -- whatever it
/// is cast at.
///
/// `Direction::ZERO` is representable, so a cast has to decide what it means.
/// Two of the four get it wrong if nobody says so: the slab test's sentinels
/// read as a hit at the far edge of the world, and the sphere's quadratic
/// reports a positive distance to an origin the ray never left. The plane and
/// the triangle already answer `None`, because a zero direction makes each of
/// their denominators zero -- which is the same branch a parallel ray takes.
#[test]
fn a_directionless_ray_hits_nothing() {
    let nowhere = Ray::new(GlobalPoint::ZERO, Direction::ZERO);
    assert!(nowhere.is_degenerate());

    assert!(nowhere.cast_against(&ball()).is_none());
    assert!(nowhere.cast_against(&ground()).is_none());
    assert!(nowhere.cast_against(&facing()).is_none());

    // And from inside a sphere, which is the case that answered a hit at the
    // origin rather than nothing at all.
    let around = Sphere::new(GlobalPoint::ZERO, metres(5.0));
    assert!(nowhere.cast_against(&around).is_none());
}
