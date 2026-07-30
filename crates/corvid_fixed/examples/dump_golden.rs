//! Prints the golden table that `tests/determinism.rs` locks in.
//!
//! Run this only when a change to the implementation is *intended* to move
//! results, and paste the output into the test. Correctness is established
//! elsewhere — `tests/trig.rs` and `tests/arithmetic.rs` check against `f64`
//! references — so this table's job is purely to make an unintended change loud.

#![allow(
    clippy::print_stdout,
    reason = "printing the table is this tool's entire purpose"
)]

use corvid_fixed::{Angle16, Angle32, Factor16, Factor32, I24F8, Signed16};

fn main() {
    print!("const GOLDEN_SIN16: &[(u16, i16)] = &[");
    for bits in [
        0_u16, 1, 1000, 8192, 16384, 20000, 32768, 40000, 49152, 60000, 65535, 12345,
    ] {
        print!("({bits}, {}), ", Angle16::from_bits(bits).sin().to_bits());
    }
    println!("];");

    print!("const GOLDEN_COS32: &[(u32, i32)] = &[");
    for bits in [
        0_u32,
        1,
        1_000_000_007,
        2_147_483_648,
        3_000_000_000,
        4_294_967_295,
    ] {
        print!("({bits}, {}), ", Angle32::from_bits(bits).cos().to_bits());
    }
    println!("];");

    print!("const GOLDEN_TAN: &[(u16, i32)] = &[");
    for bits in [1000_u16, 8192, 16000, 30000, 45000, 60000] {
        print!("({bits}, {}), ", Angle16::from_bits(bits).tan().to_bits());
    }
    println!("];");

    print!("const GOLDEN_ATAN2: &[(i64, i64, u16)] = &[");
    for (y, x) in [
        (1_i64, 3_i64),
        (-7, 2),
        (100, -100),
        (0, -1),
        (1_000_000, 3),
        (-3, -4),
        (i64::MAX, 1),
        (5, 12),
    ] {
        print!("({y}, {x}, {}), ", Angle16::atan2(y, x).to_bits());
    }
    println!("];");

    print!("const GOLDEN_MUL: &[(i32, i32, i32)] = &[");
    for (a, b) in [
        (384_i32, -64_i32),
        (1, 1),
        (-1, 1),
        (100_000, 300),
        (i32::MAX, 2),
        (12345, 6789),
    ] {
        let product = I24F8::from_bits(a).saturating_mul(I24F8::from_bits(b));
        print!("({a}, {b}, {}), ", product.to_bits());
    }
    println!("];");

    print!("const GOLDEN_SQRT: &[(i32, i32)] = &[");
    for a in [0_i32, 1, 256, 512, 1000, i32::MAX] {
        print!("({a}, {}), ", I24F8::from_bits(a).sqrt().to_bits());
    }
    println!("];");

    print!("const GOLDEN_FACTOR_MUL: &[(u16, u16, u16)] = &[");
    for (a, b) in [
        (1_u16, 1_u16),
        (32768, 32768),
        (65535, 12345),
        (60000, 60000),
        (7, 9),
    ] {
        let product = Factor16::from_bits(a).mul(Factor16::from_bits(b));
        print!("({a}, {b}, {}), ", product.to_bits());
    }
    println!("];");

    print!("const GOLDEN_LERP: &[(i32, i32, u32, i32)] = &[");
    for (a, b, t) in [
        (0_i32, 1000_i32, 1_000_000_000_u32),
        (-500, 500, 2_147_483_648),
        (7, 9, 1),
        (i32::MIN, i32::MAX, 3_000_000_000),
    ] {
        let mixed = I24F8::from_bits(a).lerp(I24F8::from_bits(b), Factor32::from_bits(t));
        print!("({a}, {b}, {t}, {}), ", mixed.to_bits());
    }
    println!("];");

    print!("const GOLDEN_SNORM_DIV: &[(i16, i16, i16)] = &[");
    for (a, b) in [
        (1_i16, 2_i16),
        (-32767, 3),
        (100, -7),
        (32767, 32767),
        (5, 1),
    ] {
        let quotient = Signed16::from_bits(a).saturating_div(Signed16::from_bits(b));
        print!("({a}, {b}, {}), ", quotient.to_bits());
    }
    println!("];");
}
