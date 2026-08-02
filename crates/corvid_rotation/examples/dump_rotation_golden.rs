//! Regenerates the golden table in `tests/determinism.rs`.
//!
//! ```sh
//! cargo run --example dump_rotation_golden
//! ```
//!
//! Run this only when a change is *meant* to move the numbers, and check them
//! against `tests/rotation32.rs` and `tests/rotation64.rs` first.

#![allow(
    clippy::print_stdout,
    reason = "regenerating a table is the entire purpose of this example"
)]
#![allow(
    clippy::many_single_char_names,
    clippy::suboptimal_flops,
    clippy::items_after_statements,
    reason = "x, y, z and w are the names this subject matter uses, and the f64 references are written as plain arithmetic so they stay independent of the implementation"
)]
use corvid_fixed::{Angle32, Pitch32};
use corvid_rotation::{Basis, FineRotation, Rotation};

/// Yaw, pitch and roll in degrees.
const POSES: &[(f64, f64, f64)] = &[
    (0.0, 0.0, 0.0),
    (90.0, 0.0, 0.0),
    (37.0, -12.0, 3.0),
    (180.0, 45.0, 0.0),
    (-120.0, 89.0, 179.0),
];

fn main() {
    println!("const GOLDEN_POSES: &[(f64, f64, f64, u32, u64)] = &[");
    for &(yaw, pitch, roll) in POSES {
        let m = Basis::from_yaw_pitch_roll(
            Angle32::from_degrees(yaw),
            Pitch32::from_degrees(pitch),
            Angle32::from_degrees(roll),
        );
        println!(
            "    ({yaw:?}, {pitch:?}, {roll:?}, {}, {}),",
            Rotation::from_basis(m).to_bits(),
            FineRotation::from_basis(m).to_bits()
        );
    }
    println!("];");
}
