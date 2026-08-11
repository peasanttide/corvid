//! What the 16-bit octahedral direction costs, and where it costs it.
//!
//! Every number this crate's README states about `OctDirection` is measured
//! here, so re-running this file is how the README is checked.
//!
//! The sampling is deliberate rather than random. The octahedral map's hard
//! places are the `z = 0` seam -- the diamond `|u| + |v| = 1`, where the upper
//! hemisphere ends and the fold begins -- and the outer edges of the square,
//! where the lower hemisphere wraps onto itself. A uniform sample of the sphere
//! lands on neither: the seam has measure zero, so a million random directions
//! miss it a million times. So the sweep walks the square rather than the
//! sphere, and the uniform sample is kept as a cross-check rather than as the
//! measurement.
//!
//! `tests/oct_codec.rs` holds the properties that are not about error size.

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
use corvid_fixed::Signed32;
use corvid_vector::{Direction, OctDirection, direction};

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

/// The worst error anywhere on the `z = 0` seam, in degrees.
///
/// A bound of its own rather than [`WORST_DEGREES`], because the seam is where
/// a fold done wrong is worst and a bound with a third of its range to spare is
/// a bound a localised regression walks through. The seam is the diamond
/// `|u| + |v| = 1`, which is where the octahedron's surface is *furthest* from
/// the origin along a coordinate axis, so a grid step subtends less angle there
/// than at a face centre -- 0.643056 degrees measured, against 0.956822 degrees globally.
const SEAM_DEGREES: f64 = 0.6431;

/// The worst error anywhere on the outer edges of the square, in degrees.
///
/// The same argument again and a tighter number still: `|u| = 1` or `|v| = 1`
/// is the far side of the fold, where one decoded coordinate is zero and the
/// direction lies in a coordinate plane. 0.451139 degrees measured.
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
            "{what}: worst {:.6} degrees over {} samples (limit {limit} degrees), at {:?}",
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
        "worst {:.6} degrees is no longer the documented 0.956822 degrees",
        worst.degrees
    );

    // The other README number. The mean is over the square rather than over the
    // sphere, which is a different question and gets a different answer below.
    assert!(
        (worst.mean() - 0.3284).abs() < 0.0005,
        "mean {:.6} degrees is no longer the documented 0.3284 degrees",
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
        "the seam now costs only {:.6} degrees, so SEAM_DEGREES is no longer its bound",
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
        "the outer edges now cost only {:.6} degrees, so EDGE_DEGREES is no longer their bound",
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
        direction(one, zero, zero),
        direction(-one, zero, zero),
        direction(zero, one, zero),
        direction(zero, -one, zero),
        direction(zero, zero, one),
        direction(zero, zero, -one),
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
        "the face centres cost only {:.6} degrees, so the worst case is not where the \
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
        "the sphere-uniform worst {:.6} degrees is no longer the documented 0.946610 degrees",
        worst.degrees
    );
    assert!(
        (worst.mean() - 0.3370).abs() < 0.0005,
        "the sphere-uniform mean {:.6} degrees is no longer the documented 0.3370 degrees",
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
