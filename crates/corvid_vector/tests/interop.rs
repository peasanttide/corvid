//! Layout guarantees and the optional integrations.
//!
//! Each feature's tests are gated on that feature, so this file compiles and
//! passes with any subset enabled. Run with `--all-features` to exercise all of
//! it.

#![allow(
    clippy::float_cmp,
    reason = "comparisons are against exactly representable references"
)]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use core::mem::{align_of, size_of};

use corvid_fixed::{I24F8, I48F16, Signed32};
use corvid_vector::{Direction, FinePoint, GlobalFinePoint, GlobalPoint};

#[test]
fn every_point_is_three_scalars_and_nothing_else() {
    assert_eq!(
        (size_of::<GlobalFinePoint>(), align_of::<GlobalFinePoint>()),
        (24, 8)
    );
    assert_eq!(
        (size_of::<GlobalPoint>(), align_of::<GlobalPoint>()),
        (12, 4)
    );
    assert_eq!((size_of::<FinePoint>(), align_of::<FinePoint>()), (12, 4));
    assert_eq!((size_of::<Direction>(), align_of::<Direction>()), (12, 4));
}

#[test]
fn default_is_the_origin() {
    assert_eq!(GlobalPoint::default(), GlobalPoint::ZERO);
    assert_eq!(GlobalFinePoint::default(), GlobalFinePoint::ZERO);
    assert_eq!(FinePoint::default(), FinePoint::ZERO);
    assert_eq!(Direction::default(), Direction::ZERO);
}

#[test]
fn equal_points_hash_equally() {
    use std::collections::HashSet;

    let mut set = HashSet::new();
    set.insert(GlobalPoint::splat(I24F8::from_f64(1.5)));
    set.insert(GlobalPoint::splat(I24F8::from_f64(1.5)));
    assert_eq!(set.len(), 1);

    // A Signed32 has two encodings of -1.0, and they denote one direction.
    let canonical = Direction::new(Signed32::MIN, Signed32::ZERO, Signed32::ZERO);
    let denormal = Direction::new(
        Signed32::from_bits(i32::MIN),
        Signed32::ZERO,
        Signed32::ZERO,
    );
    assert_eq!(canonical, denormal);
    let mut directions = HashSet::new();
    directions.insert(canonical);
    directions.insert(denormal);
    assert_eq!(directions.len(), 1);
}

#[test]
fn debug_and_display_read_as_vectors() {
    let p = GlobalPoint::new(I24F8::from_f64(1.5), I24F8::from_f64(-2.0), I24F8::ZERO);
    assert_eq!(format!("{p:?}"), "GlobalPoint(1.5, -2, 0)");
    assert_eq!(p.to_string(), "(1.5, -2, 0)");
}

#[test]
fn arrays_round_trip_through_the_trait_conversions() {
    let components = [
        I48F16::from_f64(1.0),
        I48F16::from_f64(2.0),
        I48F16::from_f64(3.0),
    ];
    let p = GlobalFinePoint::from(components);
    assert_eq!(<[I48F16; 3]>::from(p), components);
}

#[cfg(feature = "bytemuck")]
#[test]
fn points_are_plain_old_data() {
    let p = GlobalPoint::new(
        I24F8::from_bits(1),
        I24F8::from_bits(2),
        I24F8::from_bits(3),
    );
    let bytes: &[u8] = bytemuck::bytes_of(&p);
    assert_eq!(bytes.len(), 12);
    assert_eq!(bytemuck::pod_read_unaligned::<GlobalPoint>(bytes), p);
    assert_eq!(
        <GlobalPoint as bytemuck::Zeroable>::zeroed(),
        GlobalPoint::ZERO
    );
}

#[cfg(feature = "serde")]
#[test]
fn points_serialize_transparently_as_three_element_arrays() {
    // The wire size has to mean something, so assert the serialized form rather
    // than only that a round trip succeeds.
    let p = GlobalPoint::new(
        I24F8::from_bits(1),
        I24F8::from_bits(-2),
        I24F8::from_bits(3),
    );
    assert_eq!(serde_json::to_string(&p).unwrap(), "[1,-2,3]");
    assert_eq!(serde_json::from_str::<GlobalPoint>("[1,-2,3]").unwrap(), p);

    let wide = GlobalFinePoint::splat(I48F16::from_bits(1 << 40));
    let text = serde_json::to_string(&wide).unwrap();
    assert_eq!(text, "[1099511627776,1099511627776,1099511627776]");
    assert_eq!(
        serde_json::from_str::<GlobalFinePoint>(&text).unwrap(),
        wide
    );
}

#[cfg(feature = "mint")]
#[test]
fn mint_round_trips_through_f64() {
    let p = GlobalPoint::new(
        I24F8::from_f64(1.5),
        I24F8::from_f64(-2.25),
        I24F8::from_f64(3.0),
    );
    let m: mint::Vector3<f64> = p.into();
    assert_eq!(m.x, 1.5);
    assert_eq!(GlobalPoint::from(m), p);

    // f32 has enough mantissa for these values, so the narrow form round-trips
    // too.
    let near = FinePoint::new(
        corvid_fixed::I16F16::from_f64(0.5),
        corvid_fixed::I16F16::ZERO,
        corvid_fixed::I16F16::ZERO,
    );
    let m32: mint::Vector3<f32> = near.into();
    assert_eq!(FinePoint::from(m32), near);
}

#[cfg(feature = "nalgebra")]
#[test]
fn nalgebra_round_trips_through_f64() {
    let p = GlobalFinePoint::new(
        I48F16::from_f64(1.5),
        I48F16::from_f64(-2.25),
        I48F16::from_f64(3.0),
    );
    let v: nalgebra::Vector3<f64> = p.into();
    assert_eq!(v.x, 1.5);
    assert_eq!(GlobalFinePoint::from(v), p);
}
