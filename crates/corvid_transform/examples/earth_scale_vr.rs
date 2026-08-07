//! The target scenario: 10,000 objects converted world→eye at 90 Hz, with the
//! camera 6371 km from the origin.
//!
//! ```sh
//! cargo run --release --example earth_scale_vr
//! ```
//!
//! Reports nanoseconds per conversion and the fraction of the 11.1 ms frame
//! budget consumed, and quantifies the two decisions the fast path rests on:
//!
//! - **Subtracting before narrowing.** The `i64` local path against the `i128`
//!   global path.
//! - **Hoisting the basis.** Decoding the packed rotation once per frame rather
//!   than once per point.

#![allow(
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::suboptimal_flops,
    reason = "a benchmark prints numbers, and builds its inputs from raw bit patterns"
)]

use std::hint::black_box;
use std::time::Instant;

use corvid_fixed::{Angle32, I48F16, Pitch32};

use corvid_rotation::{FineRotation, Versor};

use corvid_transform::GlobalFineTransform;

use corvid_vector::{FinePoint, GlobalFinePoint, GlobalPoint};
/// Objects converted per frame.
const OBJECTS: usize = 10_000;

/// A headset's refresh rate.
const FRAME_HZ: f64 = 90.0;

/// The frame budget, in nanoseconds.
const FRAME_BUDGET_NS: f64 = 1.0e9 / FRAME_HZ;

/// Frames per timed round.
const FRAMES: usize = 20;

/// Rounds per case; the fastest is reported.
const ROUNDS: usize = 12;

/// How far the camera sits from the origin: the earth's radius.
const CAMERA_DISTANCE: f64 = 6_371_000.0;

/// A deterministic xorshift64\*, so every run measures the same scene.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A value in `-1.0 ..= 1.0`.
    fn next_unit(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 / (1u64 << 52) as f64) - 1.0
    }
}

/// Times `body` over the scene and reports nanoseconds per conversion.
fn bench(name: &str, baseline: Option<f64>, mut body: impl FnMut() -> u64) -> f64 {
    black_box(body());

    let mut best = f64::INFINITY;
    for _ in 0..ROUNDS {
        let start = Instant::now();
        let checksum = body();
        let elapsed = start.elapsed();
        black_box(checksum);
        best = best.min(elapsed.as_secs_f64() * 1e9 / (OBJECTS * FRAMES) as f64);
    }

    let frame_ns = best * OBJECTS as f64;
    let budget = frame_ns / FRAME_BUDGET_NS * 100.0;
    match baseline {
        Some(reference) => println!(
            "  {name:<38} {best:>7.2} ns   {frame_ns:>9.0} ns/frame   {budget:>5.2}% budget   {:>5.2}x",
            best / reference
        ),
        None => println!(
            "  {name:<38} {best:>7.2} ns   {frame_ns:>9.0} ns/frame   {budget:>5.2}% budget      --"
        ),
    }
    best
}

/// Folds **every** component of a near-field point into the accumulator.
///
/// Consuming one component is not enough. These conversions are `#[inline]` and
/// return a plain struct, so LLVM scalarizes the result and deletes the work
/// behind the two components nothing reads. `black_box` on the *inputs* does
/// not stop it — and the `to_fine_global` baseline happens to be immune,
/// because its `Option` depends on all three axes, so the cases compared
/// against it are the only ones that would be shortened. Measured
/// understatement without this fold: 2.8x on the `i64` path and 3.0x on the
/// `i128` one, which turns a real win from hoisting the basis into a much larger
/// reported one.
fn fold_fine(acc: u64, p: FinePoint) -> u64 {
    let [x, y, z] = p.to_array();
    acc.wrapping_add(x.to_bits() as u32 as u64)
        .wrapping_add(y.to_bits() as u32 as u64)
        .wrapping_add(z.to_bits() as u32 as u64)
}

/// Folds every component of a world-scale point. See [`fold_fine`].
fn fold_wide(acc: u64, p: GlobalFinePoint) -> u64 {
    let [x, y, z] = p.to_array();
    acc.wrapping_add(x.to_bits() as u64)
        .wrapping_add(y.to_bits() as u64)
        .wrapping_add(z.to_bits() as u64)
}

/// Folds every component of an object-scale point. See [`fold_fine`].
fn fold_coarse(acc: u64, p: GlobalPoint) -> u64 {
    let [x, y, z] = p.to_array();
    acc.wrapping_add(x.to_bits() as u32 as u64)
        .wrapping_add(y.to_bits() as u32 as u64)
        .wrapping_add(z.to_bits() as u32 as u64)
}

fn main() {
    let mut rng = Rng(0x2024_c0de_face_b00c);

    let camera = GlobalFineTransform::new(
        GlobalFinePoint::splat(I48F16::from_f64(CAMERA_DISTANCE)),
        FineRotation::from_versor(Versor::from_yaw_pitch_roll(
            Angle32::from_degrees(37.0),
            Pitch32::from_degrees(-12.0),
            Angle32::from_degrees(3.0),
        )),
    );

    // Objects scattered through a 20 km cube around the camera — the near field
    // a renderer actually draws.
    let scene: Vec<GlobalFinePoint> = (0..OBJECTS)
        .map(|_| {
            let mut axis = || I48F16::from_f64(CAMERA_DISTANCE + rng.next_unit() * 10_000.0);
            GlobalFinePoint::new(axis(), axis(), axis())
        })
        .collect();

    println!(
        "\nearth-scale VR: {OBJECTS} objects world->eye at {FRAME_HZ} Hz, \
         camera {CAMERA_DISTANCE:.0} m from the origin"
    );
    println!(
        "  {:<38} {:>10}   {:>9}   {:>13}",
        "conversion", "per point", "per frame", "of 11.1 ms"
    );

    // The shipped path: decodes the packed rotation on every call.
    let per_call = bench("GlobalFineTransform::to_fine_global", None, || {
        let mut acc = 0_u64;
        for _ in 0..FRAMES {
            for &p in &scene {
                if let Some(local) = black_box(&camera).to_fine_global(black_box(p)) {
                    acc = fold_fine(acc, local);
                }
            }
        }
        acc
    });

    // The same work with the basis decoded once per frame, which is what the
    // docs steer a hot loop toward.
    bench("hoisted basis (i64 local path)", Some(per_call), || {
        let mut acc = 0_u64;
        for _ in 0..FRAMES {
            let basis = black_box(&camera).basis();
            let origin = camera.origin();
            for &p in &scene {
                if let Some(near) = black_box(p)
                    .checked_sub(origin)
                    .and_then(GlobalFinePoint::to_fine)
                {
                    acc = fold_fine(acc, basis.unrotate_fine(near));
                }
            }
        }
        acc
    });

    // The i128 path: rotate at world width instead of subtracting first. Same
    // answer, and this is what the design exists to avoid.
    bench("hoisted basis (i128 global path)", Some(per_call), || {
        let mut acc = 0_u64;
        for _ in 0..FRAMES {
            let basis = black_box(&camera).basis();
            let origin = camera.origin();
            for &p in &scene {
                let offset = black_box(p).sub(origin);
                acc = fold_wide(acc, basis.unrotate_global_fine(offset));
            }
        }
        acc
    });

    // The coarser output, for anything not drawn at eye resolution.
    bench(
        "GlobalFineTransform::to_local_global",
        Some(per_call),
        || {
            let mut acc = 0_u64;
            for _ in 0..FRAMES {
                for &p in &scene {
                    if let Some(local) = black_box(&camera).to_local_global(black_box(p)) {
                        acc = fold_coarse(acc, local);
                    }
                }
            }
            acc
        },
    );

    // And the return trip.
    let near: Vec<FinePoint> = scene
        .iter()
        .filter_map(|&p| camera.to_fine_global(p))
        .collect();
    println!(
        "\n  eye->world over the {} points that landed in near field",
        near.len()
    );
    bench("GlobalFineTransform::to_world", Some(per_call), || {
        let mut acc = 0_u64;
        for _ in 0..FRAMES {
            for &v in &near {
                acc = fold_wide(acc, black_box(&camera).to_world(black_box(v)));
            }
        }
        acc
    });

    println!();
}
