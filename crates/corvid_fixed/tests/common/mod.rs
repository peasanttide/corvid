//! Helpers shared by the test binaries.

#![allow(
    unreachable_pub,
    dead_code,
    reason = "each test binary includes this module and uses a different subset of it"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    reason = "test helpers convert between widths freely; the values are bounded by construction"
)]

/// A deterministic xorshift64\* generator.
///
/// Used where a domain is too large to walk exhaustively. Deterministic on
/// purpose: a failure reported by CI reproduces exactly on a developer machine.
pub struct Rng(u64);

impl Rng {
    /// Seeds the generator. Any non-zero seed works.
    pub const fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    /// The next 64 bits.
    pub const fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// The next 32 bits.
    pub const fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// A value in `-1.0 ..= 1.0`.
    pub fn next_unit(&mut self) -> f64 {
        f64::from(self.next_u32()).mul_add(2.0 / f64::from(u32::MAX), -1.0)
    }
}

/// Every `i32` worth reaching for when probing boundaries.
pub const I32_EDGES: &[i32] = &[
    i32::MIN,
    i32::MIN + 1,
    -65_537,
    -65_536,
    -257,
    -256,
    -255,
    -2,
    -1,
    0,
    1,
    2,
    255,
    256,
    257,
    65_536,
    65_537,
    i32::MAX - 1,
    i32::MAX,
];

/// Reports the worst mismatch found while sweeping a domain.
#[derive(Default)]
pub struct Worst {
    /// Largest absolute error seen, in units of the output's last bit.
    pub error: i128,
    /// The input that produced it.
    pub at: i128,
    /// How many inputs were off by anything at all.
    pub inexact: u64,
    /// How many inputs were checked.
    pub checked: u64,
}

impl Worst {
    /// Folds one comparison in.
    pub const fn observe(&mut self, input: i128, actual: i128, expected: i128) {
        self.checked += 1;
        let error = (actual - expected).abs();
        if error > 0 {
            self.inexact += 1;
        }
        if error > self.error {
            self.error = error;
            self.at = input;
        }
    }

    /// Asserts the worst error stayed within `limit` last-bit units.
    pub fn assert_within(&self, limit: i128, what: &str) {
        assert!(
            self.error <= limit,
            "{what}: worst error {} bits (limit {limit}) at input {}; {} of {} inputs inexact",
            self.error,
            self.at,
            self.inexact,
            self.checked
        );
    }
}
