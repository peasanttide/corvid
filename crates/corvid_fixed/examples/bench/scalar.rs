//! The scalar rows: the tangent and the arcsine, both square roots, and the
//! multiply every fixed-point operation is built out of.

use std::hint::black_box;

use corvid_fixed::{Angle16, I2F30, I24F8, Pitch16, Signed16};

use crate::{PASSES, ROUNDS, SAMPLES, bench};

/// Times this crate's scalar arithmetic against the platform's own.
pub(crate) fn run(phases: &[u32], radians: &[f64]) {
    println!("\ntangent, arcsine, square root");
    let native_tan = bench("f64::tan", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &r in radians {
                acc = acc.wrapping_add(black_box(r).tan().to_bits());
            }
        }
        acc
    });
    bench("Angle16::tan", Some(native_tan), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let angle = Angle16::from_bits(black_box(p) as u16);
                acc = acc.wrapping_add(angle.tan().to_bits() as u64);
            }
        }
        acc
    });
    let native_asin = bench("f64::asin", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &r in radians {
                acc = acc.wrapping_add((black_box(r) / 8.0).asin().to_bits());
            }
        }
        acc
    });
    bench("Pitch16::asin", Some(native_asin), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let value = Signed16::from_bits(black_box(p) as i16);
                acc = acc.wrapping_add(Pitch16::asin(value).to_bits() as u64);
            }
        }
        acc
    });
    bench("Angle16::acos", Some(native_asin), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let value = Signed16::from_bits(black_box(p) as i16);
                acc = acc.wrapping_add(u64::from(Angle16::acos(value).to_bits()));
            }
        }
        acc
    });
    let native_sqrt = bench("f64::sqrt", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &r in radians {
                acc = acc.wrapping_add(black_box(r).sqrt().to_bits());
            }
        }
        acc
    });
    bench("I24F8::sqrt", Some(native_sqrt), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let value = I24F8::from_bits((black_box(p) >> 1) as i32);
                acc = acc.wrapping_add(value.sqrt().to_bits() as u64);
            }
        }
        acc
    });
    // The reason `rsqrt` exists rather than composing the two operations above:
    // it is one rounding instead of two, and it runs neither the `isqrt` loop
    // nor the wide divide.
    //
    // These three rows share one input domain -- values near 1, where a
    // reciprocal square root has an answer worth computing. The `sqrt` row
    // above sweeps the whole type instead, where `1/sqrt(x)` underflows to zero
    // for most of the range and would flatter `rsqrt` by measuring a branch
    // rather than the arithmetic.
    println!("\nreciprocal square root (inputs near 1.0)");
    let unit_sqrt = bench("I24F8::sqrt", Some(native_sqrt), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let value = I24F8::from_bits(((black_box(p) >> 22) | 1) as i32);
                acc = acc.wrapping_add(value.sqrt().to_bits() as u64);
            }
        }
        acc
    });
    bench("I24F8::rsqrt", Some(unit_sqrt), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let value = I24F8::from_bits(((black_box(p) >> 22) | 1) as i32);
                acc = acc.wrapping_add(value.rsqrt().to_bits() as u64);
            }
        }
        acc
    });
    bench("I24F8::sqrt().recip()", Some(unit_sqrt), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let value = I24F8::from_bits(((black_box(p) >> 22) | 1) as i32);
                acc = acc.wrapping_add(value.sqrt().recip().to_bits() as u64);
            }
        }
        acc
    });
    // The rotation decoders' own domain: a quarter to a bit over one, which is
    // where `normalize` lands its sum of squares.
    bench("I2F30::rsqrt", Some(unit_sqrt), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let value = I2F30::from_bits(((black_box(p) >> 2) | (1 << 28)) as i32);
                acc = acc.wrapping_add(value.rsqrt().to_bits() as u64);
            }
        }
        acc
    });
    bench("I2F30::sqrt().recip()", Some(unit_sqrt), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let value = I2F30::from_bits(((black_box(p) >> 2) | (1 << 28)) as i32);
                acc = acc.wrapping_add(value.sqrt().recip().to_bits() as u64);
            }
        }
        acc
    });

    println!("\nmultiplication (fixed point versus float)");
    let native_mul = bench("f64 multiply", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &r in radians {
                acc = acc.wrapping_add((black_box(r) * black_box(r)).to_bits());
            }
        }
        acc
    });
    bench("I24F8::saturating_mul", Some(native_mul), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let value = I24F8::from_bits(black_box(p) as i32);
                acc = acc.wrapping_add(value.saturating_mul(value).to_bits() as u64);
            }
        }
        acc
    });
    bench("I24F8::mul_add", Some(native_mul), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let value = I24F8::from_bits(black_box(p) as i32);
                acc = acc.wrapping_add(value.mul_add(value, value).to_bits() as u64);
            }
        }
        acc
    });
    bench("I24F8::hypot", Some(native_mul), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let value = I24F8::from_bits((black_box(p) >> 2) as i32);
                acc = acc.wrapping_add(value.hypot(value).to_bits() as u64);
            }
        }
        acc
    });
    bench("Signed16::mul", Some(native_mul), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in phases {
                let value = Signed16::from_bits(black_box(p) as i16);
                acc = acc.wrapping_add(value.mul(value).to_bits() as u64);
            }
        }
        acc
    });

    // The reciprocal square root, over positive inputs only -- the negatives
    // and zero take an early return that would otherwise flatter both tiers.
    let positives: Vec<i32> = phases.iter().map(|&p| ((p >> 1) | 1) as i32).collect();

    println!("\nreciprocal square root  ({SAMPLES} inputs x {PASSES} passes, best of {ROUNDS})");
    let native_rsqrt = bench("f64 1.0 / x.sqrt()", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in &positives {
                acc = acc.wrapping_add((1.0 / f64::from(black_box(p)).sqrt()).to_bits());
            }
        }
        acc
    });
    bench("I2F30::rsqrt", Some(native_rsqrt), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in &positives {
                let value = I2F30::from_bits(black_box(p));
                acc = acc.wrapping_add(value.rsqrt().to_bits() as u64);
            }
        }
        acc
    });
    bench("I2F30::rsqrt_fast", Some(native_rsqrt), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in &positives {
                let value = I2F30::from_bits(black_box(p));
                acc = acc.wrapping_add(value.rsqrt_fast().to_bits() as u64);
            }
        }
        acc
    });
    bench("I2F30::sqrt().recip()", Some(native_rsqrt), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for &p in &positives {
                let value = I2F30::from_bits(black_box(p));
                acc = acc.wrapping_add(value.sqrt().recip().to_bits() as u64);
            }
        }
        acc
    });
    println!();
}
