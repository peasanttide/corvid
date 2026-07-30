//! Times this crate's trigonometry against the platform's own.
//!
//! ```sh
//! cargo run --release --example bench
//! ```
//!
//! Each case runs over a fixed table of pseudo-random inputs — so branch
//! prediction cannot cheat — and reports the best of several rounds, which is the
//! most repeatable statistic on a machine with other things running. Results are
//! summed into a `black_box` so nothing is optimized away.

#![allow(
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "a benchmark's whole job is to print numbers"
)]

use std::hint::black_box;
use std::time::Instant;

use corvid_fixed::{Angle8, Angle16, Angle32, I24F8, Signed16};

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

fn main() {
    let mut rng = Rng(0x2024_c0de_face_b00c);
    let phases: Vec<u32> = (0..SAMPLES).map(|_| rng.next_u64() as u32).collect();
    let radians: Vec<f64> = phases
        .iter()
        .map(|&p| f64::from(p) / f64::from(u32::MAX) * core::f64::consts::TAU)
        .collect();
    let radians32: Vec<f32> = radians.iter().map(|&r| r as f32).collect();
    let coords: Vec<(i64, i64)> = (0..SAMPLES)
        .map(|_| (rng.next_u64() as i32 as i64, rng.next_u64() as i32 as i64))
        .collect();
    let coords_f: Vec<(f64, f64)> = coords
        .iter()
        .map(|&(y, x)| (y as f64, x as f64))
        .collect();

    println!("\nsine  ({SAMPLES} inputs x {PASSES} passes, best of {ROUNDS})");
    let native64 = bench("f64::sin", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &r in &radians {
                acc = acc.wrapping_add(black_box(r).sin().to_bits());
            }
        }
        acc
    });
    bench("f32::sin", Some(native64), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &r in &radians32 {
                acc = acc.wrapping_add(u64::from(black_box(r).sin().to_bits()));
            }
        }
        acc
    });
    bench("Angle32::sin -> Signed32", Some(native64), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in &phases {
                let angle = Angle32::from_bits(black_box(p));
                acc = acc.wrapping_add(angle.sin().to_bits() as u64);
            }
        }
        acc
    });
    bench("Angle16::sin -> Signed16", Some(native64), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in &phases {
                let angle = Angle16::from_bits(black_box(p) as u16);
                acc = acc.wrapping_add(angle.sin().to_bits() as u64);
            }
        }
        acc
    });
    bench("Angle8::sin -> Signed8", Some(native64), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in &phases {
                let angle = Angle8::from_bits(black_box(p) as u8);
                acc = acc.wrapping_add(angle.sin().to_bits() as u64);
            }
        }
        acc
    });
    bench("Angle16::sin_fast", Some(native64), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in &phases {
                let angle = Angle16::from_bits(black_box(p) as u16);
                acc = acc.wrapping_add(angle.sin_fast().to_bits() as u64);
            }
        }
        acc
    });

    println!("\nsine and cosine together");
    let native_both = bench("f64::sin_cos", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &r in &radians {
                let (s, c) = black_box(r).sin_cos();
                acc = acc.wrapping_add(s.to_bits() ^ c.to_bits());
            }
        }
        acc
    });
    bench("Angle16::sin_cos", Some(native_both), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in &phases {
                let (s, c) = Angle16::from_bits(black_box(p) as u16).sin_cos();
                acc = acc.wrapping_add((s.to_bits() as u64) ^ (c.to_bits() as u64));
            }
        }
        acc
    });

    println!("\narctangent");
    let native_atan = bench("f64::atan2", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &(y, x) in &coords_f {
                acc = acc.wrapping_add(black_box(y).atan2(black_box(x)).to_bits());
            }
        }
        acc
    });
    bench("Angle32::atan2", Some(native_atan), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &(y, x) in &coords {
                acc = acc.wrapping_add(u64::from(
                    Angle32::atan2(black_box(y), black_box(x)).to_bits(),
                ));
            }
        }
        acc
    });
    bench("Angle16::atan2", Some(native_atan), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &(y, x) in &coords {
                acc = acc.wrapping_add(u64::from(
                    Angle16::atan2(black_box(y), black_box(x)).to_bits(),
                ));
            }
        }
        acc
    });
    bench("Angle16::atan2_fast", Some(native_atan), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &(y, x) in &coords {
                acc = acc.wrapping_add(u64::from(
                    Angle16::atan2_fast(black_box(y), black_box(x)).to_bits(),
                ));
            }
        }
        acc
    });

    println!("\ntangent, arcsine, square root");
    let native_tan = bench("f64::tan", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &r in &radians {
                acc = acc.wrapping_add(black_box(r).tan().to_bits());
            }
        }
        acc
    });
    bench("Angle16::tan", Some(native_tan), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in &phases {
                let angle = Angle16::from_bits(black_box(p) as u16);
                acc = acc.wrapping_add(angle.tan().to_bits() as u64);
            }
        }
        acc
    });
    let native_sqrt = bench("f64::sqrt", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &r in &radians {
                acc = acc.wrapping_add(black_box(r).sqrt().to_bits());
            }
        }
        acc
    });
    bench("I24F8::sqrt", Some(native_sqrt), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in &phases {
                let value = I24F8::from_bits((black_box(p) >> 1) as i32);
                acc = acc.wrapping_add(value.sqrt().to_bits() as u64);
            }
        }
        acc
    });

    println!("\nmultiplication (fixed point versus float)");
    let native_mul = bench("f64 multiply", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &r in &radians {
                acc = acc.wrapping_add((black_box(r) * black_box(r)).to_bits());
            }
        }
        acc
    });
    bench("I24F8::saturating_mul", Some(native_mul), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in &phases {
                let value = I24F8::from_bits(black_box(p) as i32);
                acc = acc.wrapping_add(value.saturating_mul(value).to_bits() as u64);
            }
        }
        acc
    });
    bench("Signed16::mul", Some(native_mul), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in &phases {
                let value = Signed16::from_bits(black_box(p) as i16);
                acc = acc.wrapping_add(value.mul(value).to_bits() as u64);
            }
        }
        acc
    });
    println!();
}
