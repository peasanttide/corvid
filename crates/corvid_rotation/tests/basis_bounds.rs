//! The `i64` invariant, constructed explicitly rather than sampled for.
//!
//! Rotating a `FinePoint` by an `I2F30` basis row is `i32 × i32 → i64`, and the
//! row sum is bounded by Cauchy–Schwarz: `|m·v| ≤ |m||v|` with `|m| = 1` and
//! `|v| ≤ √3 · max|component|`. That gives `√3 × 2^30 × 2^31 = 3.99e18`
//! against `i64::MAX`'s 9.22e18 — a 131% margin.
//!
//! The bound holds **only because basis rows are unit-length**, which is what
//! the absence of a raw constructor exists to guarantee. `Signed32` would also
//! have worked here, at 15% margin; `I2F30` is chosen for the
//! shift-versus-divide reason, and the wider margin is a second benefit.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::suboptimal_flops,
    clippy::items_after_statements,
    reason = "tests reach into raw bit patterns on purpose, and their f64 references are written as plain arithmetic so they stay independent of the implementation"
)]

mod common;

use common::Rng;
use corvid_fixed::{I2F30, I16F16};
use corvid_rotation::Basis;
use corvid_vector::FinePoint;

/// `√3 · 2^30`, the largest a row of absolute entry values can sum to.
const ROW_ABS_LIMIT: i64 = 1_859_775_393;

#[test]
fn the_worst_case_row_sum_stays_inside_i64_with_room_to_spare() {
    // A row of three equal entries at 1/sqrt(3) is the unit row that maximises
    // the sum of |entries|, and (MAX, MAX, MAX) is the longest FinePoint.
    // Their product is the largest value the hot path can ever accumulate.
    let entry = i64::from(I2F30::from_f64(1.0 / 3.0f64.sqrt()).to_bits());
    let component = i64::from(I16F16::MAX.to_bits());

    let row_sum = 3 * entry * component;
    assert!(row_sum < i64::MAX, "row sum {row_sum} must fit i64");

    // The bound in closed form: sqrt(3) * 2^30 * 2^31 = 3.99e18.
    assert!(row_sum < 4_000_000_000_000_000_000, "row sum {row_sum}");
    // A 131% margin — the headroom is larger than the value itself.
    assert!(
        i64::MAX - row_sum > row_sum,
        "margin {}",
        i64::MAX - row_sum
    );

    // Signed32 would also have fitted here, at 15% margin rather than 131%.
    // Computed in f64 because the point is that it very nearly does not.
    let limit = i64::MAX as f64;
    let chosen = 3.0f64.sqrt() * f64::from(1u32 << 30) * component as f64;
    let rejected = 3.0f64.sqrt() * f64::from(i32::MAX) * component as f64;

    assert!(
        (chosen - 3.99e18).abs() < 0.01e18,
        "I2F30 worst case {chosen:e}"
    );
    assert!(
        (rejected - 7.99e18).abs() < 0.01e18,
        "Signed32 worst case {rejected:e}"
    );
    assert!(rejected < limit, "Signed32 would have fitted too");
    assert!((limit - chosen) / chosen > 1.3, "I2F30 margin");
    assert!(
        (limit - rejected) / rejected < 0.2,
        "Signed32 margin is the tight one"
    );
}

/// The worst-case orthonormal basis: its first row is three equal entries at
/// `1/√3`, which is the unit row maximising the sum of absolute entries.
fn worst_case_basis() -> Basis {
    let third = I2F30::from_f64(1.0 / 3.0f64.sqrt());
    let half = I2F30::from_f64(1.0 / 2.0f64.sqrt());
    let sixth = I2F30::from_f64(1.0 / 6.0f64.sqrt());
    let two_sixths = I2F30::from_f64(2.0 / 6.0f64.sqrt());

    Basis::from_rows([
        [third, third, third],
        [I2F30::ZERO, half, -half],
        // Negated against the obvious choice, which has determinant -1 and is
        // therefore a reflection rather than a rotation.
        [-two_sixths, sixth, sixth],
    ])
    .expect("orthonormal with determinant +1")
}

#[test]
fn rotating_the_longest_fine_point_by_that_basis_saturates_rather_than_wrapping() {
    let basis = worst_case_basis();
    let corner = FinePoint::splat(I16F16::MAX);
    let rotated = basis.rotate_fine(corner);

    // The first row sums to sqrt(3) * MAX, which overflows FinePoint — so the
    // saturating form clamps and the checked form says so. What must *not*
    // happen is a wrap, which is what the i64 bound rules out.
    assert_eq!(rotated.x(), I16F16::MAX);
    assert_eq!(basis.checked_rotate_fine(corner), None);

    // A point inside the safe ball rotates without clamping at all.
    let safe = FinePoint::splat(I16F16::from_f64(10_000.0));
    assert!(basis.checked_rotate_fine(safe).is_some());
}

#[test]
fn from_rows_rejects_anything_that_is_not_a_rotation() {
    let one = I2F30::ONE;
    let zero = I2F30::ZERO;

    // A scaled row is exactly the case that would saturate silently.
    let scaled = I2F30::from_f64(1.9);
    assert_eq!(
        Basis::from_rows([[scaled, zero, zero], [zero, one, zero], [zero, zero, one]]),
        None
    );

    // A short row, which under-rotates rather than overflowing, is still not a
    // rotation.
    let short = I2F30::from_f64(0.5);
    assert_eq!(
        Basis::from_rows([[short, zero, zero], [zero, one, zero], [zero, zero, one]]),
        None
    );

    // Non-orthogonal rows.
    assert_eq!(
        Basis::from_rows([[one, zero, zero], [one, zero, zero], [zero, zero, one]]),
        None
    );

    // A reflection: orthonormal, but determinant -1.
    assert_eq!(
        Basis::from_rows([
            [one, zero, zero],
            [zero, one, zero],
            [zero, zero, I2F30::from_f64(-1.0)],
        ]),
        None
    );

    // All zeros.
    assert_eq!(Basis::from_rows([[zero; 3]; 3]), None);

    // The identity survives.
    assert_eq!(
        Basis::from_rows([[one, zero, zero], [zero, one, zero], [zero, zero, one]]),
        Some(Basis::IDENTITY)
    );
}

#[test]
fn from_rows_accepts_what_the_crate_itself_produces() {
    // The tolerance has to be loose enough for a rotation that has been through
    // the codecs and back, or it would reject the crate's own output.
    let mut rng = Rng::new(0xF20_2015);
    for _ in 0..20_000 {
        let basis = common::random_basis(&mut rng);
        assert!(
            Basis::from_rows(basis.to_rows()).is_some(),
            "from_rows rejected a basis this crate produced: {basis:?}"
        );
    }
}

#[test]
fn saturation_is_unreachable_from_any_public_constructor() {
    // Every rotation this crate can produce has unit rows, so the bound holds
    // for all of them. Sample the whole space and check the row sums directly.
    let mut rng = Rng::new(0x0B05_1500);
    for _ in 0..50_000 {
        let basis = common::random_basis(&mut rng);
        for row in basis.to_rows() {
            let sum: i64 = row.iter().map(|e| i64::from(e.to_bits()).abs()).sum();
            // sqrt(3) * 2^30, plus a few last bits of rounding slack.
            assert!(
                sum <= ROW_ABS_LIMIT + 16,
                "row sum {sum} exceeds sqrt(3) * 2^30 = {ROW_ABS_LIMIT}"
            );
        }
    }
}

#[test]
fn partial_sums_obey_the_same_bound_so_accumulation_order_is_free() {
    // Cauchy-Schwarz with sqrt(2) in place of sqrt(3): any two of the three
    // terms are bounded too, so there is no ordering hazard and no need to fix
    // an accumulation order.
    let basis = worst_case_basis();
    let component = i64::from(I16F16::MAX.to_bits());
    for row in basis.to_rows() {
        for (a, b) in [(0, 1), (0, 2), (1, 2)] {
            let partial =
                (i64::from(row[a].to_bits()).abs() + i64::from(row[b].to_bits()).abs()) * component;
            assert!(partial < i64::MAX, "partial sum {partial}");
            // sqrt(2) * 2^30 * 2^31 = 3.26e18.
            assert!(partial < 3_300_000_000_000_000_000, "partial sum {partial}");
        }
    }
}
