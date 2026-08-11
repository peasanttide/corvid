//! Which way is right, which way is up, and which way is far.
//!
//! Every test here is about a convention rather than a value, because a
//! convention is what a renderer gets wrong silently: a projection with two
//! rows exchanged still puts a cube on the screen, upside down or mirrored, and
//! nothing but an assertion notices. Each one is written so that swapping the
//! pair it names moves it.
//!
//! These moved here from `corvid_render` with the matrices themselves, and they
//! are the reason the move is checkable: not one of them needs a device, so the
//! whole of the fixed-point-to-`f32` boundary is testable in a crate that has
//! never heard of `wgpu`.

#![allow(
    clippy::float_cmp,
    clippy::suboptimal_flops,
    reason = "the comparisons here are against bounds and signs rather than against exact quotients; and the matrix-vector products below are written out because they are the thing being checked, so a fused multiply-add that rounded differently from the shader would make this test about something else"
)]
#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_camera::{Eye, matrix::model, matrix::projection, matrix::view};
use corvid_fixed::{Angle16, I16F16, I48F16};
use corvid_glm::{Mat4, nalgebra::Vector4};
use corvid_rotation::{FineRotation, Rotation};
use corvid_shape::Frustum;
use corvid_transform::{FineTransform, Transform};
use corvid_vector::{FinePoint, GlobalFinePoint, globalfinepoint};

/// A camera at the origin, facing +Y with +Z up, spanning 90 degrees vertically from
/// 0.1 m to 100 m.
const fn looking_forward() -> FineTransform {
    FineTransform::new(
        FinePoint::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO).to_global_fine(),
        FineRotation::IDENTITY,
    )
}

/// The 90 degrees frustum the tests below look through.
const fn square_lens() -> Frustum {
    Frustum::perspective(
        Angle16::from_degrees(90.0),
        I16F16::from_f64(0.1),
        I16F16::from_f64(100.0),
    )
}

/// The whole view-projection that camera implies, at `aspect`.
fn seen(camera: FineTransform, aspect: f32) -> Mat4 {
    projection(square_lens(), aspect) * view(camera)
}

/// Where a point in the camera's own space lands, in normalized device
/// coordinates.
///
/// A plain matrix-vector product, because the matrix is already in the order a
/// shader reads it -- column-major, so no transpose stands between the matrix
/// and the product.
fn ndc(camera: FineTransform, point: [f32; 3]) -> [f32; 3] {
    let clip = seen(camera, 2.0) * Vector4::new(point[0], point[1], point[2], 1.0);
    [clip.x / clip.w, clip.y / clip.w, clip.z / clip.w]
}

#[test]
fn forward_is_down_the_middle_and_the_planes_land_on_zero_and_one() {
    // The near and far planes are what a depth test is defined against, so they
    // are exact rather than approximate. A projection whose depth row reads
    // from the wrong axis puts both of these somewhere else.
    let camera = looking_forward();
    let near = ndc(camera, [0.0, 0.1, 0.0]);
    let far = ndc(camera, [0.0, 100.0, 0.0]);
    assert_eq!([near[0], near[1]], [0.0, 0.0]);
    // Not exact, and the tolerance is the reason `f32` is only ever reached on
    // this side of the boundary: the near plane's depth is a difference of two
    // nearly equal products, so it cancels to a few parts in ten thousand
    // rather than to zero. A row read from the wrong axis misses by a whole
    // plane rather than by that.
    assert!(near[2].abs() < 1e-3, "the near plane is at {}", near[2]);
    assert!(
        (far[2] - 1.0).abs() < 1e-3,
        "the far plane is at {}",
        far[2]
    );
}

#[test]
fn right_is_right_and_up_is_up() {
    // The one that catches the y/z exchange between the workspace's camera
    // convention and wgpu's clip convention. Both signs are asserted, because a
    // swap sends each of them to the other axis rather than to zero, and one
    // assertion alone would still pass.
    let camera = looking_forward();
    let right = ndc(camera, [1.0, 10.0, 0.0]);
    let up = ndc(camera, [0.0, 10.0, 1.0]);
    assert!(
        right[0] > 0.0 && right[1].abs() < 1e-6,
        "right is {right:?}"
    );
    assert!(up[1] > 0.0 && up[0].abs() < 1e-6, "up is {up:?}");
}

#[test]
fn a_wide_viewport_sees_more_sideways_rather_than_zooming() {
    // Vertical field of view: changing the aspect must move the horizontal
    // extent and leave the vertical one alone. A projection that divided the
    // wrong row would zoom when a player widened the window.
    let camera = looking_forward();
    let square = {
        let m = seen(camera, 1.0);
        (m[(0, 0)], m[(1, 2)])
    };
    let wide = {
        let m = seen(camera, 2.0);
        (m[(0, 0)], m[(1, 2)])
    };
    assert!(wide.0 < square.0, "widening did not widen");
    assert_eq!(wide.1, square.1, "widening changed the vertical extent");
}

#[test]
fn an_instance_is_placed_relative_to_the_eye() {
    // The whole point of the camera-relative path: the same instance seen from
    // two cameras a kilometre apart differs by exactly that kilometre, and an
    // instance under the eye is at the origin.
    let here = Transform::new(
        FinePoint::new(I16F16::ZERO, I16F16::from_f64(10.0), I16F16::ZERO).to_global(),
        Rotation::IDENTITY,
    );
    let eye = FinePoint::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO).to_global_fine();
    let far = FinePoint::new(I16F16::ZERO, I16F16::from_f64(1000.0), I16F16::ZERO).to_global_fine();

    assert_eq!(model(here, eye)[(1, 3)], 10.0);
    assert_eq!(model(here, far)[(1, 3)], -990.0);
}

/// The claim the one-formula projection is built on: an orthographic box is the
/// `slope == 0` case rather than an approximation of a very long frustum.
#[test]
fn an_orthographic_box_does_not_narrow_with_distance() {
    // The one property that separates the two projections, asserted rather than
    // assumed: a perspective divide shrinks a thing as it recedes and an
    // orthographic one does not. Both are checked, because a `projection` that
    // had a perspective row in it would pass the second half alone.
    let near = [1.0f32, 5.0, 0.0];
    let far = [1.0f32, 50.0, 0.0];
    let flat = projection(
        Frustum::orthographic(
            I16F16::from_f64(10.0),
            I16F16::from_f64(0.1),
            I16F16::from_f64(100.0),
        ),
        2.0,
    );
    let deep = projection(square_lens(), 2.0);

    let at = |m: Mat4, p: [f32; 3]| m * Vector4::new(p[0], p[1], p[2], 1.0);
    let x = |m: Mat4, p: [f32; 3]| {
        let c = at(m, p);
        c.x / c.w
    };
    assert_eq!(x(flat, near), x(flat, far), "the orthographic box narrowed");
    assert!(
        x(deep, far).abs() < x(deep, near).abs(),
        "the perspective frustum did not narrow",
    );

    // And the near and far planes still land where a depth test expects them,
    // which is what would break if the depth row were scaled by the vertical
    // extent along with the other two.
    let z = |m: Mat4, p: [f32; 3]| {
        let c = at(m, p);
        c.z / c.w
    };
    assert!(z(flat, [0.0, 0.1, 0.0]).abs() < 1e-6);
    assert!((z(flat, [0.0, 100.0, 0.0]) - 1.0).abs() < 1e-6);
}

/// Both frustums, through one formula, against the values separate perspective
/// and orthographic code would produce. This is the test that one formula is
/// answerable to.
#[test]
fn one_formula_reproduces_both_of_the_projections_it_replaced() {
    let aspect = 16.0 / 9.0;
    let (near, far) = (I16F16::from_f64(0.1), I16F16::from_f64(100.0));

    // Perspective: `focal / aspect` across, `focal` up, and `w` reading the
    // forward axis -- the shape the hand-written matrix had, up to the overall
    // scale a homogeneous divide cancels.
    let lens = Frustum::perspective(Angle16::from_degrees(90.0), near, far);
    let m = projection(lens, aspect);
    let slope = lens.slope.to_f32();
    assert!((m[(0, 0)] * slope * aspect - 1.0).abs() < 1e-3, "{m}");
    assert_eq!(m[(1, 2)], 1.0);
    assert_eq!(m[(3, 1)], slope, "w must read the forward axis");
    assert_eq!(
        m[(3, 3)],
        0.0,
        "a perspective frustum has its apex at the eye"
    );

    // Orthographic: `w` constant, so nothing narrows.
    let box_ = Frustum::orthographic(I16F16::from_f64(10.0), near, far);
    let m = projection(box_, aspect);
    assert_eq!(m[(3, 1)], 0.0, "a box must not read the forward axis");
    assert_eq!(m[(3, 3)], 5.0, "w is the half-height, which is constant");
}

#[test]
fn a_frustum_with_no_depth_produces_no_infinities() {
    // Nothing checks a game's frustum before it reaches this module, so a
    // degenerate one arrives. Every entry has to stay finite: an infinity in a
    // matrix makes a NaN out of every vertex it touches, and a device
    // rasterises a NaN into whatever it likes.
    let degenerate = projection(
        Frustum::perspective(
            Angle16::from_degrees(90.0),
            I16F16::from_f64(5.0),
            I16F16::from_f64(5.0),
        ),
        2.0,
    );
    for cell in (degenerate * view(looking_forward())).iter() {
        assert!(cell.is_finite(), "a degenerate frustum produced {cell}");
    }
}

#[test]
fn a_field_of_view_of_nothing_produces_no_infinities() {
    // The other degenerate frustum, and the one the depth guard does not cover:
    // a field of view of zero has no extent at all, so the `w` row would be
    // zero and every vertex a NaN. A full turn is checked too, because
    // `Angle16` wraps and a game that adds its way round to 360 degrees arrives
    // at exactly the same zero.
    for fov in [Angle16::ZERO, Angle16::from_degrees(360.0)] {
        let degenerate = projection(
            Frustum::perspective(fov, I16F16::from_f64(0.1), I16F16::from_f64(100.0)),
            2.0,
        );
        for cell in (degenerate * view(looking_forward())).iter() {
            assert!(
                cell.is_finite(),
                "a field of view of {fov:?} produced {cell}"
            );
        }
    }
}

#[test]
fn a_millimetre_survives_ten_thousand_kilometres_from_the_origin() {
    // The whole claim of `Eye`, in the one place it can be checked without a
    // device: a point ten metres from a camera 1e7 m out still resolves to
    // millimetres, and the naive absolute `f32` it replaces does not.
    const OUT: i32 = 10_000_000;
    let camera = FineTransform::new(globalfinepoint(OUT, 0, 0), FineRotation::IDENTITY);
    let seen = Eye::new(camera, Frustum::default(), 1.0);
    assert_eq!(seen.coarse, [OUT, 0, 0]);

    // Ten metres in front of it, and a millimetre further.
    let ten = at(OUT, 10.0);
    let and_a_bit = at(OUT, 10.001);

    // The offset a game hands its vertex stage: an integer subtraction, then an
    // `f32`. This is where the precision is won, and the two are a millimetre
    // apart to a part in ten thousand.
    let [near, far] = [ten, and_a_bit].map(|point| relative(point, seen.coarse));
    assert!(
        ((far[1] - near[1]) - 0.001).abs() < 1e-4,
        "the offsets are {near:?} and {far:?}",
    );

    // And the matrix carries the difference through to clip space rather than
    // flattening it: two points a millimetre apart do not land on one pixel's
    // worth of the same number.
    let (a, b) = (through(seen.clip, near), through(seen.clip, far));
    assert_ne!(a, b, "a millimetre vanished between the eye and clip space");

    // The half this exists to avoid. The same two positions as absolute `f32`
    // are the same `f32`: 1e7 is past the twenty-four bits of mantissa, so a
    // metre is the step there and a millimetre is nothing.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the truncation is the thing being demonstrated"
    )]
    let absolute = |offset: f64| (f64::from(OUT) + offset) as f32;
    assert_eq!(
        absolute(10.0),
        absolute(10.001),
        "an absolute f32 was precise enough after all, which would make this test pointless",
    );
}

/// A point `offset` metres along `+Y` from `(out, 0, 0)`.
fn at(out: i32, offset: f64) -> GlobalFinePoint {
    globalfinepoint(out, 0, 0)
        + GlobalFinePoint::new(I48F16::ZERO, I48F16::from_f64(offset), I48F16::ZERO)
}

/// What a game hands its vertex stage: a world position minus the eye's coarse
/// position, in integers, narrowed to `f32` only afterwards.
#[allow(
    clippy::cast_precision_loss,
    reason = "the sub-metre remainder is sixteen bits against an f32 mantissa's twenty-four, which is the precision this test exists to demonstrate is kept"
)]
fn relative(point: GlobalFinePoint, coarse: [i32; 3]) -> [f32; 3] {
    let components = point.to_array();
    let mut out = [0.0f32; 3];
    for (axis, component) in out.iter_mut().enumerate() {
        let bits = components[axis].to_bits() - (i64::from(coarse[axis]) << I48F16::FRAC_BITS);
        *component = bits as f32 / (1u32 << I48F16::FRAC_BITS) as f32;
    }
    out
}

/// A point through a clip matrix, as clip-space `x`, `y`, `z`.
///
/// No transpose. The matrix is stored the way a shader reads it, which is the
/// whole of what the column-major change bought.
fn through(clip: Mat4, point: [f32; 3]) -> [f32; 3] {
    let out = clip * Vector4::new(point[0], point[1], point[2], 1.0);
    [out.x, out.y, out.z]
}
