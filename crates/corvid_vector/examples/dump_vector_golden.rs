//! Regenerates the golden tables in `tests/determinism.rs`.
//!
//! ```sh
//! cargo run --example dump_vector_golden
//! ```
//!
//! The tables exist so a refactor that quietly changes a result fails loudly.
//! Run this only when a change is *meant* to move them, and check the new
//! numbers against `tests/vector.rs`'s `f64` references before pasting them in.

#![allow(
    clippy::print_stdout,
    reason = "regenerating a table is the entire purpose of this example"
)]

use corvid_fixed::I24F8;
use corvid_vector::GlobalPoint;

/// The inputs the tables are built from, as raw component bit patterns.
const INPUTS: &[[i32; 3]] = &[
    [256, 0, 0],
    [0, -256, 0],
    [768, 1024, 3072],
    [1, 1, 1],
    [-2_147_483_647, 1, 0],
];

/// Builds a point from raw component bits.
const fn point(bits: [i32; 3]) -> GlobalPoint {
    GlobalPoint::new(
        I24F8::from_bits(bits[0]),
        I24F8::from_bits(bits[1]),
        I24F8::from_bits(bits[2]),
    )
}

fn main() {
    println!("const GOLDEN_NORMALIZE: &[([i32; 3], [i32; 3])] = &[");
    for &input in INPUTS {
        // Every input above is non-zero, so this branch is the only one taken;
        // `expect` is denied workspace-wide, so say it with a match.
        match point(input).normalize() {
            Some(unit) => println!(
                "    ({input:?}, [{}, {}, {}]),",
                unit.x().to_bits(),
                unit.y().to_bits(),
                unit.z().to_bits()
            ),
            None => println!("    ({input:?}, /* zero vector, no direction */),"),
        }
    }
    println!("];\n");

    println!("const GOLDEN_LENGTH: &[([i32; 3], i32)] = &[");
    for &input in INPUTS {
        println!("    ({input:?}, {}),", point(input).length().to_bits());
    }
    println!("];");
}
