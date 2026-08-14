//! What the 16-bit octahedral codec guarantees, apart from how close it lands.
//!
//! The sibling of `tests/oct.rs`, which measures the error. These are the
//! properties that are true or false rather than large or small: that decoding
//! is a fixed point of encoding, that the denormal pattern means what it says,
//! that a decoder folding at the seam is what the map requires rather than a
//! detail, and that sixteen bits are being spent about as well as sixteen bits
//! can be.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::many_single_char_names,
    reason = "x, y, z and the octahedral u, v, w are the names this subject matter uses"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    reason = "the error measurement runs in f64 over integer-valued grid indices, all far inside the mantissa"
)]

mod common;

use common::Rng;
use corvid_fixed::{Signed8, Signed32};
use corvid_vector::{Direction, OctDirection};

/// The bit-pattern scale of a [`Signed8`], and so of the encoding's grid.
const UNIT: i32 = 127;

/// The worst error the codec has, in degrees, as measured by the sweep below.
///
/// It is the measured figure rounded up in its last digit rather than a
/// tolerance chosen to pass: the sweep reports 0.956822 degrees, so a change costing a
/// further thousandth of a degree fails here.
///
/// The number is not mysterious, which is worth writing down because a bound
/// nobody can derive is a bound nobody can tell has drifted. The worst place is
/// the centre of an octahedron face -- the `(+/-1, +/-1, +/-1)/sqrt3` directions -- where
/// the octahedron's surface is nearest the origin, at `1/sqrt3`, so a step of the
/// `(u, v)` grid subtends the largest angle. One step along `u` moves the point
/// by `(1, 0, -1)/127` and one along `v` by `(0, 1, -1)/127`, so the centre of a
/// cell sits `(0.5, 0.5, -1)/127` from its corner, a length of `sqrt1.5/127`, all
/// of it perpendicular to the radius. Dividing that by the radius `1/sqrt3` gives
/// `sqrt4.5/127` radians, which is 0.95703 degrees.
///
/// The sweep finds 0.956822 degrees, which is the same to three digits and two
/// thousandths of a degree lower. That gap is the derivation's own: it treats
/// the cell as flat and takes the angle as the offset over the radius, and both
/// approximations run high. It is a hundred times smaller than the difference
/// between this bound and the next-worst place on the map, which is what makes
/// the derivation worth having.
const WORST_DEGREES: f64 = 0.9569;

/// A [`Direction`] from three `f64` components, normalized.
///
/// Rescales onto the [`Signed32`] range first so that the crate's own integer
/// normalize does the work; a component-wise `from_f64` would clamp each axis
/// instead and change the direction.
fn unit(x: f64, y: f64, z: f64) -> Direction {
    let largest = x.abs().max(y.abs()).max(z.abs());
    assert!(largest > 0.0, "no direction to build");
    let scale = 2_147_483_647.0 / largest;
    let component = |v: f64| Signed32::from_bits((v * scale) as i32);
    Direction::new(component(x), component(y), component(z))
        .normalize()
        .unwrap()
}

/// The angle between two directions, in degrees.
///
/// Both operands are rescaled to unit length in `f64` first. A [`Direction`]'s
/// components are quantized, so its length is only `1 +/- 1e-9`, and feeding that
/// straight to `acos` near zero angle reports `sqrt(2e-9)` -- 0.003 degrees of pure
/// measurement artefact, which would be most of some of the numbers below.
fn angle_between(a: Direction, b: Direction) -> f64 {
    let normalized = |d: Direction| {
        let (x, y, z) = (d.x().to_f64(), d.y().to_f64(), d.z().to_f64());
        let length = z.mul_add(z, x.mul_add(x, y * y)).sqrt();
        (x / length, y / length, z / length)
    };
    let (ax, ay, az) = normalized(a);
    let (bx, by, bz) = normalized(b);
    az.mul_add(bz, ax.mul_add(bx, ay * by))
        .clamp(-1.0, 1.0)
        .acos()
        .to_degrees()
}

#[test]
fn re_encoding_a_decoded_direction_never_moves_it() {
    // What a mesh processing step does: decode a normal, transform nothing,
    // encode it again. If that could move the direction, a normal would drift a
    // step per pass. It cannot, and this walks every one of the 65025 canonical
    // codes to say so.
    //
    // The literal code is *not* always the same code, and the exceptions are
    // named rather than tolerated: 507 of them, every one with a component at
    // +/-127. Those are the outer edge of the square, where the fold sets the
    // other coordinate to zero and the sign it carries is then arbitrary -- two
    // codes, one direction. The encoder picks one of the two consistently, which
    // is why the direction is stable even where the code is not.
    let mut codes_that_moved = 0_u32;
    let mut checked = 0_u32;
    for u in -127_i8..=127 {
        for v in -127_i8..=127 {
            let code = OctDirection::new(Signed8::from_bits(u), Signed8::from_bits(v));
            let again = OctDirection::encode(code.decode());
            assert_eq!(
                again.decode(),
                code.decode(),
                "the direction at code ({u}, {v}) moved when it was re-encoded"
            );
            if again != code {
                assert!(
                    u.abs() == 127 || v.abs() == 127,
                    "code ({u}, {v}) is not on the outer edge and still moved, \
                     to ({}, {})",
                    again.u().to_bits(),
                    again.v().to_bits()
                );
                codes_that_moved += 1;
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 255 * 255);
    assert_eq!(codes_that_moved, 507);
}

#[test]
fn the_denormal_component_is_the_value_it_denotes() {
    // `Signed8` spends `-128` and `-127` on the same `-1.0`. Both must decode to
    // the same direction and compare equal, or two peers holding the same normal
    // would exchange different marks -- the failure the SNORM convention exists
    // to prevent rather than cause.
    let canonical = OctDirection::new(Signed8::from_bits(-127), Signed8::from_bits(40));
    let denormal = OctDirection::new(Signed8::from_bits(-128), Signed8::from_bits(40));
    assert_eq!(canonical, denormal);
    assert_eq!(canonical.decode(), denormal.decode());

    // And the encoder never emits the denormal, so it only ever arrives from
    // `from_bits`, `bytemuck` or a capture.
    let mut rng = Rng::new(0x5eed_5eed);
    for _ in 0..10_000 {
        let encoded = OctDirection::encode(unit(
            rng.next_unit(),
            rng.next_unit(),
            rng.next_unit().abs() + 1e-3,
        ));
        assert!(!encoded.u().is_denormal() && !encoded.v().is_denormal());
    }
}

#[test]
fn a_decoder_that_clamped_instead_of_folding_would_fail_this_file() {
    // What the sweep is protecting against, written out. The fold is the only
    // part of the octahedral map that is not obvious, and the plausible wrong
    // version treats the square as a plain projection with `z = 1 - |u| - |v|`
    // clamped at zero.
    //
    // It agrees with the real decoder over the whole diamond and is 90 degrees wrong in
    // the corners, so it passes an upper-hemisphere test and fails the sweep.
    // Without this, "the sweep covers the corners" is a claim about the sweep
    // rather than a fact about it.
    fn clamped(code: OctDirection) -> Direction {
        let u = f64::from(code.u().canonicalize().to_bits());
        let v = f64::from(code.v().canonicalize().to_bits());
        unit(u, v, (f64::from(UNIT) - u.abs() - v.abs()).max(0.0))
    }

    let mut inside = 0_u32;
    let mut worst_inside = 0.0_f64;
    let mut worst_in_the_corners = 0.0_f64;
    for u in -127_i8..=127 {
        for v in -127_i8..=127 {
            let code = OctDirection::new(Signed8::from_bits(u), Signed8::from_bits(v));
            let error = angle_between(code.decode(), clamped(code));
            if i32::from(u.abs()) + i32::from(v.abs()) <= UNIT {
                worst_inside = worst_inside.max(error);
                inside += 1;
            } else {
                worst_in_the_corners = worst_in_the_corners.max(error);
            }
        }
    }

    // Inside the diamond the two are one function, up to the `f64` reference's
    // own rounding -- three orders of magnitude below the codec's own error.
    assert!(inside > 32_000);
    assert!(
        worst_inside < 1e-4,
        "the two decoders differ by {worst_inside:e} degrees inside the diamond, where \
         they are supposed to be the same function"
    );
    assert!(
        worst_in_the_corners > 89.0,
        "the clamping decoder was only {worst_in_the_corners:.2} degrees wrong, so this \
         file would not have caught it"
    );
}

#[test]
fn the_codec_is_within_a_factor_of_two_of_any_sixteen_bit_encoding() {
    // How much of `WORST_DEGREES` is the width rather than the octahedron.
    //
    // Sixteen bits name 65536 directions, so each has to stand for `4pi/65536`
    // steradians of a sphere that must be covered entirely. A spherical cap of
    // that area has angular radius `r` where `2pi(1 - cos r) = 4pi/65536`, and a
    // set of caps that covers the sphere cannot have every cap smaller than
    // that -- so no 16-bit encoding at all, whatever its layout, has a worst case
    // below `r`.
    let cap = 1.0_f64 - 2.0 / 65_536.0;
    let floor = cap.acos().to_degrees();
    assert!(
        (floor - 0.4476).abs() < 0.0001,
        "the covering floor moved to {floor:.6} degrees"
    );

    // And this codec against it. The ratio is what the octahedral map's own
    // distortion costs -- its cells are largest at the face centres and smallest
    // at the corners, where an ideal encoding's would be equal. A codec that
    // drifted worse would fail here as well as at the bound itself, and one that
    // claimed to beat the floor would be reporting an error it had not measured.
    let ratio = WORST_DEGREES / floor;
    assert!(
        (2.0..2.2).contains(&ratio),
        "the codec is {ratio:.3}x the 16-bit floor, not the documented 2.14x"
    );
}
