//! Locks the results down.
//!
//! Two independent checks, following `corvid_fixed/tests/determinism.rs`:
//!
//! 1. **Const equals runtime.** Every operation is evaluated twice — once by
//!    rustc's const interpreter at compile time, once by the CPU at run time —
//!    and the two must agree bit for bit. These are separate implementations of
//!    the arithmetic, so agreement is real evidence that nothing here depends on
//!    host floating-point behavior.
//!
//! 2. **Golden tables.** Fixed inputs paired with the exact bits they produce.
//!    Correctness is established in `tests/vector.rs` against `f64`; these exist
//!    so that a refactor which quietly changes a result fails loudly.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_sign_loss,
    reason = "the golden tables are raw bit patterns, converted as such"
)]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use std::hint::black_box;

use common::Rng;
use corvid_fixed::{Factor32, I24F8, I48F16, Signed32};
use corvid_vector::{Direction, FinePoint, GlobalFinePoint, GlobalPoint};

/// `(x, y, z)` component bits, then the `Direction` bits `normalize` produces.
const GOLDEN_NORMALIZE: &[([i32; 3], [i32; 3])] = &[
    ([256, 0, 0], [2_147_483_647, 0, 0]),
    ([0, -256, 0], [0, -2_147_483_647, 0]),
    ([768, 1024, 3072], [495_573_149, 660_764_199, 1_982_292_598]),
    ([1, 1, 1], [1_239_850_261, 1_239_850_261, 1_239_850_261]),
    // The last row is the extreme-ratio case: a component 2^31 times smaller
    // than the largest is below what a shift-based rescale can carry, and it
    // lands on zero rather than on Direction's own last bit. That is the cost
    // of shifting instead of dividing, and it is one last bit of a unit vector.
    ([-2_147_483_647, 1, 0], [-2_147_483_647, 0, 0]),
];

/// `(x, y, z)` component bits, then the `length` bits they produce.
const GOLDEN_LENGTH: &[([i32; 3], i32)] = &[
    ([256, 0, 0], 256),
    ([0, -256, 0], 256),
    ([768, 1024, 3072], 3328),
    ([1, 1, 1], 2),
    ([-2_147_483_647, 1, 0], 2_147_483_647),
];

const fn point(bits: [i32; 3]) -> GlobalPoint {
    GlobalPoint::new(
        I24F8::from_bits(bits[0]),
        I24F8::from_bits(bits[1]),
        I24F8::from_bits(bits[2]),
    )
}

#[test]
fn normalize_matches_its_golden_table() {
    for &(input, expected) in GOLDEN_NORMALIZE {
        let unit = point(input).normalize().expect("non-zero");
        let actual = [unit.x().to_bits(), unit.y().to_bits(), unit.z().to_bits()];
        assert_eq!(actual, expected, "normalize({input:?})");
    }
}

#[test]
fn length_matches_its_golden_table() {
    for &(input, expected) in GOLDEN_LENGTH {
        assert_eq!(
            point(input).length().to_bits(),
            expected,
            "length({input:?})"
        );
    }
}

#[test]
fn const_evaluation_agrees_with_runtime() {
    const A: GlobalPoint = GlobalPoint::new(
        I24F8::from_f64(3.0),
        I24F8::from_f64(4.0),
        I24F8::from_f64(12.0),
    );
    const B: GlobalPoint = GlobalPoint::new(
        I24F8::from_f64(-1.5),
        I24F8::from_f64(0.25),
        I24F8::from_f64(7.0),
    );

    const LENGTH: I24F8 = A.length();
    const SQUARED: u64 = A.length_squared();
    const DOT: i128 = A.dot(B);
    const CROSS: GlobalPoint = A.cross(B);
    const UNIT: Option<Direction> = A.normalize();
    const DISTANCE: I24F8 = A.distance(B);
    const LERP: GlobalPoint = A.lerp(B, Factor32::from_f64(0.25));
    const WIDE: GlobalFinePoint = A.to_global_fine();
    const NEAR: Option<FinePoint> = A.to_fine();

    let (a, b) = (black_box(A), black_box(B));
    assert_eq!(LENGTH, a.length());
    assert_eq!(SQUARED, a.length_squared());
    assert_eq!(DOT, a.dot(b));
    assert_eq!(CROSS, a.cross(b));
    assert_eq!(UNIT, a.normalize());
    assert_eq!(DISTANCE, a.distance(b));
    assert_eq!(LERP, a.lerp(b, Factor32::from_f64(0.25)));
    assert_eq!(WIDE, a.to_global_fine());
    assert_eq!(NEAR, a.to_fine());
}

#[test]
fn const_evaluation_agrees_with_runtime_at_the_other_widths() {
    const WIDE: GlobalFinePoint = GlobalFinePoint::new(
        I48F16::from_f64(6_371_000.0),
        I48F16::from_f64(-1.0e13),
        I48F16::from_f64(0.001),
    );
    const WIDE_LENGTH: I48F16 = WIDE.length();
    const WIDE_UNIT: Option<Direction> = WIDE.normalize();
    const DIRECTION: Direction = Direction::new(Signed32::MAX, Signed32::ZERO, Signed32::ZERO);
    const DIRECTION_LENGTH: Signed32 = DIRECTION.length();

    let wide = black_box(WIDE);
    assert_eq!(WIDE_LENGTH, wide.length());
    assert_eq!(WIDE_UNIT, wide.normalize());
    assert_eq!(DIRECTION_LENGTH, black_box(DIRECTION).length());
    assert_eq!(DIRECTION_LENGTH, Signed32::MAX);
}

#[test]
fn the_same_seed_gives_the_same_stream_of_results() {
    // The whole determinism claim in one sentence: identical inputs produce
    // identical bits, run after run.
    let checksum = |seed: u64| {
        let mut rng = Rng::new(seed);
        let mut acc = 0u64;
        for _ in 0..10_000 {
            let p = common::random_global_point(&mut rng, 100_000.0);
            acc = acc
                .wrapping_mul(31)
                .wrapping_add(p.length().to_bits() as u64);
            if let Some(unit) = p.normalize() {
                acc = acc.wrapping_mul(31).wrapping_add(unit.x().to_bits() as u64);
            }
        }
        acc
    };
    // Pinned rather than compared to a rerun of itself: a rerun proves only
    // that the function is a function. A change that moves every result
    // alike — a rounding rule, an `rsqrt` retune — fails here.
    assert_eq!(
        checksum(0xD37_E4A1),
        11_369_903_297_625_141_325,
        "the sequence changed"
    );
}

// --- normalize_fast --------------------------------------------------------

/// `(x, y, z)` component bits, then the bits `normalize_fast` produces.
///
/// Pinned for the same reason the exact table above is, and more so: an
/// approximation has no external definition to check against, so this table is
/// what a retuned seed or a dropped Newton step would have to disagree with.
const GOLDEN_NORMALIZE_FAST: &[([i32; 3], [i32; 3])] = &[
    // The axis-aligned cases are taken by hand before the `rsqrt`, so both
    // tiers agree on them exactly.
    ([256, 0, 0], [2_147_483_647, 0, 0]),
    ([0, -256, 0], [0, -2_147_483_647, 0]),
    // About 1.0e-5 below the exact tier's [495_573_149, 660_764_199,
    // 1_982_292_598] — a common-mode scale error, which is why all three
    // components move by the same fraction rather than by the same amount.
    ([768, 1024, 3072], [495_568_032, 660_757_376, 1_982_272_129]),
    // The diagonal, against the exact 1_239_850_261.
    ([1, 1, 1], [1_239_828_377, 1_239_828_377, 1_239_828_377]),
    // The extreme-ratio row: the tiny second component keeps the sum of
    // squares just off the `0.25` the reduction special-cases, so this one goes
    // through the approximation and lands two last bits short of full scale
    // where the exact tier reaches it.
    ([-2_147_483_647, 1, 0], [-2_147_483_645, 0, 0]),
];

#[test]
fn normalize_fast_matches_its_golden_table() {
    for &(input, expected) in GOLDEN_NORMALIZE_FAST {
        let unit = point(input).normalize_fast().expect("non-zero");
        let actual = [unit.x().to_bits(), unit.y().to_bits(), unit.z().to_bits()];
        assert_eq!(actual, expected, "normalize_fast({input:?})");
    }
}

#[test]
fn normalize_fast_is_available_in_const_context() {
    const A: GlobalPoint = GlobalPoint::new(
        I24F8::from_bits(768),
        I24F8::from_bits(1024),
        I24F8::from_bits(3072),
    );
    const UNIT_FAST: Option<Direction> = A.normalize_fast();

    assert_eq!(UNIT_FAST, A.normalize_fast());
    assert!(UNIT_FAST.is_some());
}
