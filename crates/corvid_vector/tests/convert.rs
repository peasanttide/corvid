//! Every width conversion, at the boundary and either side of it.

#![allow(
    clippy::float_cmp,
    reason = "comparisons are against exactly representable references"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    reason = "these tests feed edge-case bit patterns through narrowing casts on purpose"
)]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::Rng;
use corvid_fixed::{I16F16, I24F8, I48F16, Signed32};
use corvid_vector::{Direction, FinePoint, GlobalFinePoint, GlobalPoint};

#[test]
fn global_fine_to_fine_is_bit_exact_never_rounded() {
    // Both types carry 16 fractional bits, so this is `i64 as i32` after a
    // bounds test. Walk the whole fractional space of one component.
    for frac in 0..=u16::MAX {
        let bits = (1234i64 << 16) | i64::from(frac);
        let wide = GlobalFinePoint::new(
            I48F16::from_bits(bits),
            I48F16::from_bits(-bits),
            I48F16::ZERO,
        );
        let near = wide.to_fine().expect("in range");
        assert_eq!(i64::from(near.x().to_bits()), bits);
        assert_eq!(i64::from(near.y().to_bits()), -bits);
        assert_eq!(near.z(), I16F16::ZERO);
    }
}

#[test]
fn narrowing_returns_none_exactly_at_the_boundary() {
    let at_top = GlobalFinePoint::new(
        I48F16::from_bits(i64::from(i32::MAX)),
        I48F16::ZERO,
        I48F16::ZERO,
    );
    assert!(at_top.to_fine().is_some());

    let one_past_top = GlobalFinePoint::new(
        I48F16::from_bits(i64::from(i32::MAX) + 1),
        I48F16::ZERO,
        I48F16::ZERO,
    );
    assert_eq!(one_past_top.to_fine(), None);

    let at_bottom = GlobalFinePoint::new(
        I48F16::from_bits(i64::from(i32::MIN)),
        I48F16::ZERO,
        I48F16::ZERO,
    );
    assert!(at_bottom.to_fine().is_some());

    let one_past_bottom = GlobalFinePoint::new(
        I48F16::from_bits(i64::from(i32::MIN) - 1),
        I48F16::ZERO,
        I48F16::ZERO,
    );
    assert_eq!(one_past_bottom.to_fine(), None);

    // Any axis out of range fails the whole conversion.
    let z_only = GlobalFinePoint::new(
        I48F16::ZERO,
        I48F16::ZERO,
        I48F16::from_bits(i64::from(i32::MAX) + 1),
    );
    assert_eq!(z_only.to_fine(), None);
}

#[test]
fn widening_is_exact() {
    let mut rng = Rng::new(0x00C0_FFEE);
    for _ in 0..20_000 {
        let p = common::random_global_point(&mut rng, 8_000_000.0);
        let wide = p.to_global_fine();
        assert_eq!(wide.x().to_f64(), p.x().to_f64());
        assert_eq!(wide.y().to_f64(), p.y().to_f64());
        assert_eq!(wide.z().to_f64(), p.z().to_f64());
        assert_eq!(wide.to_global(), Some(p));

        // FinePoint widens by pattern alone.
        if let Some(near) = p.to_fine() {
            let widened = near.to_global_fine();
            assert_eq!(widened.x().to_bits(), i64::from(near.x().to_bits()));
            assert_eq!(widened.to_fine(), Some(near));
        }
    }
}

#[test]
fn fine_to_global_is_total_and_rounds_rather_than_truncating() {
    // I16F16's whole range fits inside I24F8, so this never fails...
    let extreme = FinePoint::splat(I16F16::MAX);
    let coarse = extreme.to_global();
    assert!((coarse.x().to_f64() - I16F16::MAX.to_f64()).abs() < 0.004);

    let extreme_low = FinePoint::splat(I16F16::MIN);
    assert!((extreme_low.to_global().x().to_f64() - I16F16::MIN.to_f64()).abs() < 0.004);

    // ...and half a coarse step rounds away from zero, not toward it.
    let half_step = FinePoint::new(I16F16::from_bits(128), I16F16::ZERO, I16F16::ZERO);
    assert_eq!(half_step.to_global().x().to_bits(), 1);

    let negative_half = FinePoint::new(I16F16::from_bits(-128), I16F16::ZERO, I16F16::ZERO);
    assert_eq!(negative_half.to_global().x().to_bits(), -1);

    // Just under half rounds to zero.
    let under = FinePoint::new(I16F16::from_bits(127), I16F16::ZERO, I16F16::ZERO);
    assert_eq!(under.to_global().x().to_bits(), 0);
}

#[test]
fn global_to_fine_is_exact_in_resolution_and_only_range_checked() {
    let inside = GlobalPoint::splat(I24F8::from_f64(30_000.0));
    let near = inside.to_fine().expect("30 km is inside +-32.7 km");
    assert_eq!(near.x().to_f64(), inside.x().to_f64());

    let outside = GlobalPoint::splat(I24F8::from_f64(40_000.0));
    assert_eq!(outside.to_fine(), None);

    // The boundary is where I16F16 runs out, to the last bit. `i32::MAX / 256`
    // is the largest `I24F8` whose `<< 8` still fits an `i32`, so it is the
    // last pattern that converts; one step past it must not.
    let at_edge = GlobalPoint::new(I24F8::from_bits(i32::MAX / 256), I24F8::ZERO, I24F8::ZERO);
    assert_eq!(
        at_edge.to_fine().map(|p| p.x().to_bits()),
        Some((i32::MAX / 256) << 8),
        "the last representable pattern must convert, exactly"
    );
    let past_edge = GlobalPoint::new(
        I24F8::from_bits(i32::MAX / 256 + 1),
        I24F8::ZERO,
        I24F8::ZERO,
    );
    assert_eq!(past_edge.to_fine(), None, "one bit past the edge must not");
    let just_inside = GlobalPoint::new(I24F8::from_f64(32_767.99), I24F8::ZERO, I24F8::ZERO);
    assert!(just_inside.to_fine().is_some());
    let just_outside = GlobalPoint::new(I24F8::from_f64(32_768.01), I24F8::ZERO, I24F8::ZERO);
    assert_eq!(just_outside.to_fine(), None);
}

#[test]
fn a_direction_converts_to_a_unit_length_offset() {
    let x = Direction::new(Signed32::MAX, Signed32::ZERO, Signed32::ZERO);
    assert_eq!(
        x.to_fine(),
        FinePoint::new(I16F16::ONE, I16F16::ZERO, I16F16::ZERO)
    );
    assert_eq!(
        x.to_global(),
        GlobalPoint::new(I24F8::ONE, I24F8::ZERO, I24F8::ZERO)
    );
    assert_eq!(
        x.to_global_fine(),
        GlobalFinePoint::new(I48F16::ONE, I48F16::ZERO, I48F16::ZERO)
    );

    let negative = Direction::new(Signed32::ZERO, Signed32::MIN, Signed32::ZERO);
    assert_eq!(negative.to_fine().y(), -I16F16::ONE);
}

#[test]
fn the_trait_conversions_agree_with_the_inherent_ones() {
    let p = GlobalPoint::splat(I24F8::from_f64(12.5));
    assert_eq!(GlobalFinePoint::from(p), p.to_global_fine());
    assert_eq!(FinePoint::try_from(p).ok(), p.to_fine());

    let wide = GlobalFinePoint::splat(I48F16::from_f64(1.0e13));
    assert!(FinePoint::try_from(wide).is_err());
    assert!(GlobalPoint::try_from(wide).is_err());
}

#[test]
fn conversions_are_available_in_const_context() {
    const COARSE: GlobalPoint = GlobalPoint::splat(I24F8::ONE);
    const WIDE: GlobalFinePoint = COARSE.to_global_fine();
    const BACK: Option<GlobalPoint> = WIDE.to_global();
    const NEAR: Option<FinePoint> = COARSE.to_fine();

    assert_eq!(BACK, Some(COARSE));
    assert_eq!(NEAR, Some(FinePoint::splat(I16F16::ONE)));
    assert_eq!(WIDE, GlobalFinePoint::splat(I48F16::ONE));
}

/// `OutOfRange` is an error in the trait sense, not merely a type named one.
///
/// This crate is `no_std`, so the trait is `core::error::Error` — which is what
/// `thiserror` derives when its `std` feature is off. A build that turned that
/// feature on would name `std::error::Error` instead and fail to compile here
/// rather than quietly dropping the impl, and this is what says so.
#[test]
fn out_of_range_is_an_error() {
    const fn assert_error<E: core::error::Error>() {}
    assert_error::<corvid_vector::OutOfRange>();
}
