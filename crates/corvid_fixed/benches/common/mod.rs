//! The inputs every benchmark in this crate runs over.
//!
//! One table, built once, and every row walks the whole of it. That is what
//! stops branch prediction from cheating: a benchmark over one value measures
//! how fast a branch predictor learns it, and a benchmark over a spread of
//! values measures the function.

#![allow(
    unreachable_pub,
    dead_code,
    reason = "each benchmark binary includes this module and uses a different subset of it"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    reason = "the inputs are bit patterns being reinterpreted at other widths on purpose, which is what makes one table serve every row"
)]

/// How many inputs each row runs over.
///
/// A `u64` rather than a `usize` because it is what Criterion's throughput
/// takes, and every use of it here is a count rather than an index.
pub const SAMPLES: u64 = 4096;

/// A deterministic xorshift64\* generator.
///
/// Deterministic so that two runs measure the same work. A benchmark whose
/// inputs moved between runs would report the difference between the inputs.
pub struct Rng(u64);

impl Rng {
    /// The generator every table here is built from.
    pub const fn new() -> Self {
        Self(0x2024_c0de_face_b00c)
    }

    /// The next value in the sequence.
    pub const fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

/// The tables the rows run over, each the same length and each derived from the
/// same draw, so a fixed-point row and the `f64` row beside it see the same
/// values.
pub struct Inputs {
    /// Raw phases, read as an angle at whichever width a row wants.
    pub phases: Vec<u32>,
    /// The same phases as radians, for the platform's own trigonometry.
    pub radians: Vec<f64>,
    /// The same again, narrowed, for the `f32` rows.
    pub radians32: Vec<f32>,
    /// Coordinate pairs for the arctangent.
    pub coords: Vec<(i64, i64)>,
    /// The same pairs as floats.
    pub coords_f: Vec<(f64, f64)>,
    /// The same pairs at the width the fast arctangent takes.
    pub coords32: Vec<(i32, i32)>,
    /// Positive bit patterns, for the roots.
    pub positives: Vec<i32>,
}

impl Inputs {
    /// Draws every table.
    pub fn new() -> Self {
        let mut rng = Rng::new();
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
        let coords_f = coords.iter().map(|&(y, x)| (y as f64, x as f64)).collect();
        let coords32 = coords.iter().map(|&(y, x)| (y as i32, x as i32)).collect();
        let positives = phases.iter().map(|&p| ((p >> 1) | 1) as i32).collect();
        Self {
            phases,
            radians,
            radians32,
            coords,
            coords_f,
            coords32,
            positives,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::new()
    }
}
