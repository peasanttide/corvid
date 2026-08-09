//! The 16-bit octahedral direction: what it costs, and where it costs it.
//!
//! Every number this crate's README states about `OctDirection` is measured
//! here, so re-running this file is how the README is checked.
//!
//! The sampling is deliberate rather than random. The octahedral map's hard
//! places are the `z = 0` seam -- the diamond `|u| + |v| = 1`, where the upper
//! hemisphere ends and the fold begins -- and the outer edges of the square,
//! where the lower hemisphere wraps onto itself. A uniform sample of the sphere
//! lands on neither: the seam has measure zero, so a million random directions
//! miss it a million times, and a decoder that clamped instead of folding would
//! pass a random-only test while being 90 deg wrong in a corner. So the sweep walks
//! the *square* on a grid four times finer than the encoding's own, which puts
//! samples exactly on the seam, exactly on the outer edges, exactly at the
//! corners, and exactly at the centre of every quantization cell -- which is
//! where the worst case turns out to live.

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

/// How many sample points per encoding step. Four puts samples on the seam, on
/// the outer edges and at every cell centre at once.
const OVERSAMPLE: i32 = 4;

/// One end of the swept grid, in oversampled steps.
const REACH: i32 = UNIT * OVERSAMPLE;

/// The worst error the codec has, in degrees, as measured by the sweep below.
///
/// It is the measured figure rounded up in its last digit rather than a
/// tolerance chosen to pass: the sweep reports 0.956822 deg, so a change costing a
/// further thousandth of a degree fails here.
///
/// The number is not mysterious, which is worth writing down because a bound
/// nobody can derive is a bound nobody can tell has drifted. The worst place is
/// the centre of an octahedron face -- the `(+/-1, +/-1, +/-1)/sqrt(3)` directions -- where
/// the octahedron's surface is nearest the origin, at `1/sqrt(3)`, so a step of the
/// `(u, v)` grid subtends the largest angle. One step along `u` moves the point
/// by `(1, 0, -1)/127` and one along `v` by `(0, 1, -1)/127`, so the centre of a
/// cell sits `(0.5, 0.5, -1)/127` from its corner, a length of `sqrt(1.5)/127`, all
/// of it perpendicular to the radius. Dividing that by the radius `1/sqrt(3)` gives
/// `sqrt(4.5)/127` radians, which is 0.95703 deg.
///
/// The sweep finds 0.956822 deg, which is the same to three digits and two
/// thousandths of a degree lower. That gap is the derivation's own: it treats
/// the cell as flat and takes the angle as the offset over the radius, and both
/// approximations run high. It is a hundred times smaller than the difference
/// between this bound and the next-worst place on the map, which is what makes
/// the derivation worth having.
const WORST_DEGREES: f64 = 0.9569;

/// The worst error anywhere on the `z = 0` seam, in degrees.
///
/// A bound of its own rather than [`WORST_DEGREES`], because the seam is where
/// a fold done wrong is worst and a bound with a third of its range to spare is
/// a bound a localised regression walks through. The seam is the diamond
/// `|u| + |v| = 1`, which is where the octahedron's surface is *furthest* from
/// the origin along a coordinate axis, so a grid step subtends less angle there
/// than at a face centre -- 0.643056 deg measured, against 0.956822 deg globally.
const SEAM_DEGREES: f64 = 0.6431;

/// The worst error anywhere on the outer edges of the square, in degrees.
///
/// The same argument again and a tighter number still: `|u| = 1` or `|v| = 1`
/// is the far side of the fold, where one decoded coordinate is zero and the
/// direction lies in a coordinate plane. 0.451139 deg measured.
const EDGE_DEGREES: f64 = 0.4512;

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
/// straight to `acos` near zero angle reports `sqrt(2e-9)` -- 0.003 deg of pure
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

/// The direction the octahedral point `(u, v)` denotes, computed in `f64`.
///
/// The reference decode, written the way the literature writes it, so that the
/// integer implementation is checked against the maths rather than against
/// itself.
fn from_square(u: f64, v: f64) -> Direction {
    let w = 1.0 - u.abs() - v.abs();
    if w < 0.0 {
        unit(
            (1.0 - v.abs()) * if u < 0.0 { -1.0 } else { 1.0 },
            (1.0 - u.abs()) * if v < 0.0 { -1.0 } else { 1.0 },
            w,
        )
    } else {
        unit(u, v, w)
    }
}

/// The worst round-trip error over a set of directions, and where it was.
#[derive(Default)]
struct Worst {
    degrees: f64,
    total: f64,
    count: u64,
    at: Option<Direction>,
}

impl Worst {
    fn observe(&mut self, sample: Direction) {
        let error = angle_between(sample, OctDirection::encode(sample).decode());
        self.total += error;
        self.count += 1;
        if error > self.degrees {
            self.degrees = error;
            self.at = Some(sample);
        }
    }

    fn mean(&self) -> f64 {
        self.total / self.count as f64
    }

    fn assert_within(&self, limit: f64, what: &str) {
        assert!(
            self.degrees <= limit,
            "{what}: worst {:.6} deg over {} samples (limit {limit} deg), at {:?}",
            self.degrees,
            self.count,
            self.at
        );
    }
}

#[test]
fn the_worst_error_over_the_whole_square_is_the_documented_one() {
    let mut worst = Worst::default();
    for i in -REACH..=REACH {
        for j in -REACH..=REACH {
            let u = f64::from(i) / f64::from(REACH);
            let v = f64::from(j) / f64::from(REACH);
            worst.observe(from_square(u, v));
        }
    }

    // 1017 x 1017: four samples per encoding step along each axis.
    assert_eq!(worst.count, 1_034_289);
    worst.assert_within(WORST_DEGREES, "swept square");

    // Bounded from below too. A codec that got *better* here would leave the
    // README describing something this crate no longer does, which is the same
    // kind of stale as one that got worse -- and it would mean the derivation
    // above had stopped applying.
    assert!(
        worst.degrees > 0.9560,
        "worst {:.6} deg is no longer the documented 0.956822 deg",
        worst.degrees
    );

    // The other README number. The mean is over the square rather than over the
    // sphere, which is a different question and gets a different answer below.
    assert!(
        (worst.mean() - 0.3284).abs() < 0.0005,
        "mean {:.6} deg is no longer the documented 0.3284 deg",
        worst.mean()
    );
}

#[test]
fn the_seam_and_the_outer_edges_are_inside_the_same_bound() {
    // The `z = 0` great circle, which is the diamond |u| + |v| = 1 -- where the
    // upper hemisphere ends and the fold begins. Every point on it lies on a
    // boundary between two octahedron faces.
    let mut seam = Worst::default();
    for i in 0..4 * REACH {
        let t = f64::from(i % REACH) / f64::from(REACH);
        let (u, v) = match i / REACH {
            0 => (1.0 - t, t),
            1 => (-t, 1.0 - t),
            2 => (t - 1.0, -t),
            _ => (t, t - 1.0),
        };
        assert!((u.abs() + v.abs() - 1.0).abs() < 1e-12, "not on the seam");
        seam.observe(from_square(u, v));
    }
    assert_eq!(seam.count, 4 * REACH as u64);
    seam.assert_within(SEAM_DEGREES, "z = 0 seam");
    // And that the bound is the seam's own rather than one with room in it: the
    // measured worst has to be close under it, or a later change could move the
    // seam a long way and still pass.
    assert!(
        seam.degrees > 0.6420,
        "the seam now costs only {:.6} deg, so SEAM_DEGREES is no longer its bound",
        seam.degrees
    );

    // The outer edges of the square, where the lower hemisphere wraps onto
    // itself: |u| = 1 or |v| = 1. Each of these is continuous only because of
    // the fold.
    let mut edges = Worst::default();
    for i in -REACH..=REACH {
        let t = f64::from(i) / f64::from(REACH);
        edges.observe(from_square(1.0, t));
        edges.observe(from_square(-1.0, t));
        edges.observe(from_square(t, 1.0));
        edges.observe(from_square(t, -1.0));
    }
    edges.assert_within(EDGE_DEGREES, "square edges");
    assert!(
        edges.degrees > 0.4500,
        "the outer edges now cost only {:.6} deg, so EDGE_DEGREES is no longer their bound",
        edges.degrees
    );

    // And a band either side of the seam, at offsets unrelated to the encoding
    // grid, so the samples are not all at points the quantizer represents
    // exactly.
    let mut band = Worst::default();
    for i in 0..1_000 {
        let t = f64::from(i) / 1_000.0;
        for offset in [-3e-3, -1e-4, 0.0, 1e-4, 3e-3] {
            band.observe(from_square(t, 1.0 - t + offset));
            band.observe(from_square(-t, t - 1.0 + offset));
        }
    }
    // The band contains the seam itself -- offset zero is on it -- so it is held
    // to the seam's bound and not to a looser one.
    band.assert_within(SEAM_DEGREES, "seam band");
}

#[test]
fn the_six_axes_survive_exactly_and_the_face_centres_are_the_worst_case() {
    // The six axes are corners of the octahedron, so they land on grid points of
    // the square and the round trip is exact rather than merely inside the
    // bound. A quantizer biased by half a step would lose this while still
    // passing the sweep.
    let (zero, one) = (Signed32::ZERO, Signed32::MAX);
    for axis in [
        Direction::new(one, zero, zero),
        Direction::new(-one, zero, zero),
        Direction::new(zero, one, zero),
        Direction::new(zero, -one, zero),
        Direction::new(zero, zero, one),
        Direction::new(zero, zero, -one),
    ] {
        assert_eq!(
            OctDirection::encode(axis).decode(),
            axis,
            "the axis {axis:?} did not survive the round trip exactly"
        );
    }

    // The eight body diagonals are the octahedron's face centres, which is where
    // `WORST_DEGREES` was derived. Landing on the centre of a face is not the
    // same as landing on the centre of a *cell*, so these eight are not the
    // worst case themselves -- but they are the direction the worst case is near,
    // and a codec whose error peaked somewhere else entirely would fail here.
    let mut diagonals = Worst::default();
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            for z in [-1.0f64, 1.0] {
                diagonals.observe(unit(x, y, z));
            }
        }
    }
    assert_eq!(diagonals.count, 8);
    diagonals.assert_within(WORST_DEGREES, "face centres");
    assert!(
        diagonals.degrees > 0.6,
        "the face centres cost only {:.6} deg, so the worst case is not where the \
         derivation says it is",
        diagonals.degrees
    );
}

#[test]
fn a_uniform_sample_of_the_sphere_agrees_with_the_sweep() {
    // A second sampling with nothing in common with the grid: rejection sampling
    // in the cube, which reaches directions no grid point names. It cannot find
    // the worst case -- that is what the sweep is for -- but it says the sweep's
    // grid is not quietly the only thing that works, and its mean is the honest
    // answer to "what does this cost a normal", since a mesh's normals are
    // spread over the sphere rather than over the square.
    let mut rng = Rng::new(0x00c7_a10c_7a10_c7a1);
    let mut worst = Worst::default();
    while worst.count < 200_000 {
        let (x, y, z) = (rng.next_unit(), rng.next_unit(), rng.next_unit());
        let square = z.mul_add(z, x.mul_add(x, y * y));
        if !(1e-6..=1.0).contains(&square) {
            continue;
        }
        worst.observe(unit(x, y, z));
    }
    // Both README numbers for this sampling, at the sample count the README
    // quotes: a mean nobody bounded from above would be satisfied by a codec
    // that had become uniformly worse, and a worst nobody asserted at all is a
    // figure in a table that no test produces.
    assert_eq!(worst.count, 200_000);
    worst.assert_within(0.9467, "random directions");
    assert!(
        worst.degrees > 0.9400,
        "the sphere-uniform worst {:.6} deg is no longer the documented 0.946610 deg",
        worst.degrees
    );
    assert!(
        (worst.mean() - 0.3370).abs() < 0.0005,
        "the sphere-uniform mean {:.6} deg is no longer the documented 0.3370 deg",
        worst.mean()
    );

    // And this is why the sweep exists rather than only this test. `|u| + |v| =
    // 1` is `z = 0`, so a direction is near the seam when its `z` is inside one
    // encoding step of zero. Counting how rarely that happens is what says a
    // random sample cannot be trusted to have visited the fold.
    let mut near_seam = 0_u32;
    for _ in 0..200_000 {
        let (x, y, z) = (rng.next_unit(), rng.next_unit(), rng.next_unit());
        let square = z.mul_add(z, x.mul_add(x, y * y));
        if (1e-6..=1.0).contains(&square) && (z / square.sqrt()).abs() < 1.0 / f64::from(UNIT) {
            near_seam += 1;
        }
    }
    assert!(
        near_seam < 200_000 / 50,
        "{near_seam} of 200000 random directions landed within a step of the \
         seam, which would make the targeted sweep redundant"
    );
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
    // It agrees with the real decoder over the whole diamond and is 90 deg wrong in
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
        "the two decoders differ by {worst_inside:e} deg inside the diamond, where \
         they are supposed to be the same function"
    );
    assert!(
        worst_in_the_corners > 89.0,
        "the clamping decoder was only {worst_in_the_corners:.2} deg wrong, so this \
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
        "the covering floor moved to {floor:.6} deg"
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
