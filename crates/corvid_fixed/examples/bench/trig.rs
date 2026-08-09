//! The trigonometry rows: sine, sine and cosine together, and the arctangent.

use std::hint::black_box;

use corvid_fixed::{Angle8, Angle16, Angle32};

use crate::{PASSES, ROUNDS, SAMPLES, bench};

/// Times this crate's trigonometry against the platform's own.
pub(crate) fn run(
    phases: &[u32],
    radians: &[f64],
    radians32: &[f32],
    coords: &[(i64, i64)],
    coords_f: &[(f64, f64)],
) {
    println!("\nsine  ({SAMPLES} inputs x {PASSES} passes, best of {ROUNDS})");
    let native64 = bench("f64::sin", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &r in radians {
                acc = acc.wrapping_add(black_box(r).sin().to_bits());
            }
        }
        acc
    });
    bench("f32::sin", Some(native64), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &r in radians32 {
                acc = acc.wrapping_add(u64::from(black_box(r).sin().to_bits()));
            }
        }
        acc
    });
    bench("Angle32::sin -> Signed32", Some(native64), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let angle = Angle32::from_bits(black_box(p));
                acc = acc.wrapping_add(angle.sin().to_bits() as u64);
            }
        }
        acc
    });
    bench("Angle16::sin -> Signed16", Some(native64), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let angle = Angle16::from_bits(black_box(p) as u16);
                acc = acc.wrapping_add(angle.sin().to_bits() as u64);
            }
        }
        acc
    });
    bench("Angle8::sin -> Signed8", Some(native64), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let angle = Angle8::from_bits(black_box(p) as u8);
                acc = acc.wrapping_add(angle.sin().to_bits() as u64);
            }
        }
        acc
    });
    bench("Angle16::sin_fast", Some(native64), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
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
            for &r in radians {
                let (s, c) = black_box(r).sin_cos();
                acc = acc.wrapping_add(s.to_bits() ^ c.to_bits());
            }
        }
        acc
    });
    bench("Angle16::sin_cos", Some(native_both), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
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
            for &(y, x) in coords_f {
                acc = acc.wrapping_add(black_box(y).atan2(black_box(x)).to_bits());
            }
        }
        acc
    });
    bench("Angle32::atan2", Some(native_atan), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &(y, x) in coords {
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
            for &(y, x) in coords {
                acc = acc.wrapping_add(u64::from(
                    Angle16::atan2(black_box(y), black_box(x)).to_bits(),
                ));
            }
        }
        acc
    });
    // The fast path takes 32-bit coordinates so it can run on a GPU; `coords`
    // was built from `i32` values, so narrowing it back loses nothing.
    let coords_32: Vec<(i32, i32)> = coords.iter().map(|&(y, x)| (y as i32, x as i32)).collect();
    bench("Angle16::atan2_fast", Some(native_atan), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &(y, x) in &coords_32 {
                acc = acc.wrapping_add(u64::from(
                    Angle16::atan2_fast(black_box(y), black_box(x)).to_bits(),
                ));
            }
        }
        acc
    });
}
