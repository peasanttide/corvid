//! Times this crate's trigonometry against the platform's own.
//!
//! ```sh
//! cargo run --release --example bench
//! ```
//!
//! Each case runs over a fixed table of pseudo-random inputs -- so branch
//! prediction cannot cheat -- and reports the best of several rounds, which is the
//! most repeatable statistic on a machine with other things running. Results are
//! summed into a `black_box` so nothing is optimized away.

#![allow(
    clippy::redundant_pub_crate,
    reason = "the workspace enables unreachable_pub, which wants the opposite of what this nursery lint suggests for a private module's items"
)]
#![allow(
    clippy::print_stdout,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::missing_const_for_fn,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    reason = "a benchmark prints numbers, and reaches into raw bit patterns to build its inputs"
)]

use std::hint::black_box;
use std::time::Instant;

/// Inputs per round.
const SAMPLES: usize = 4096;

/// Rounds per case; the fastest one is reported.
const ROUNDS: usize = 12;

/// Passes over the sample table per round.
const PASSES: usize = 32;

/// A deterministic xorshift64\*, so every run measures the same work.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

/// Times `body` over the sample table and returns nanoseconds per operation.
fn bench(name: &str, baseline: Option<f64>, mut body: impl FnMut() -> u64) -> f64 {
    // Warm the caches and let any frequency scaling settle.
    black_box(body());

    let mut best = f64::INFINITY;
    for _ in 0..ROUNDS {
        let start = Instant::now();
        let checksum = body();
        let elapsed = start.elapsed();
        black_box(checksum);
        let per_op = elapsed.as_secs_f64() * 1e9 / (SAMPLES * PASSES) as f64;
        best = best.min(per_op);
    }

    match baseline {
        Some(reference) => {
            println!("  {name:<34} {best:>8.2} ns   {:>6.2}x", best / reference);
        }
        None => println!("  {name:<34} {best:>8.2} ns        --"),
    }
    best
}
mod scalar;
mod trig;

fn main() {
    let mut rng = Rng(0x2024_c0de_face_b00c);
    let phases: Vec<u32> = (0..SAMPLES).map(|_| rng.next_u64() as u32).collect();
    let radians: Vec<f64> = phases
        .iter()
        .map(|&p| f64::from(p) / f64::from(u32::MAX) * core::f64::consts::TAU)
        .collect();
    let radians32: Vec<f32> = radians.iter().map(|&r| r as f32).collect();
    let coords: Vec<(i64, i64)> = (0..SAMPLES)
        .map(|_| {
            (
                i64::from(rng.next_u64() as i32),
                i64::from(rng.next_u64() as i32),
            )
        })
        .collect();
    let coords_f: Vec<(f64, f64)> = coords.iter().map(|&(y, x)| (y as f64, x as f64)).collect();
    trig::run(&phases, &radians, &radians32, &coords, &coords_f);
    scalar::run(&phases, &radians);
    println!();
}
