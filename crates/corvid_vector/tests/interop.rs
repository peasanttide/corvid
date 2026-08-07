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

use corvid_fixed::{I24F8, I48F16, Signed8, Signed32};
use corvid_vector::{Direction, FinePoint, GlobalFinePoint, GlobalPoint, OctDirection};

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

    // Sixteen bits, byte-aligned, which is the whole reason the packed normal
    // exists — and it is `wgpu`'s `Snorm8x2` only while both of these hold.
    assert_eq!(
        (size_of::<OctDirection>(), align_of::<OctDirection>()),
        (2, 1)
    );
}

#[test]
fn default_is_the_origin() {
    assert_eq!(GlobalPoint::default(), GlobalPoint::ZERO);
    assert_eq!(GlobalFinePoint::default(), GlobalFinePoint::ZERO);
    assert_eq!(FinePoint::default(), FinePoint::ZERO);
    assert_eq!(Direction::default(), Direction::ZERO);
}

#[test]
fn the_zero_packed_normal_points_up() {
    // A zeroed vertex buffer is a real thing, so the all-zero pattern had better
    // name a direction rather than nothing. It is +Z, and `Default`, `UP` and
    // the decode all have to agree about that.
    assert_eq!(OctDirection::default(), OctDirection::UP);
    assert_eq!(
        OctDirection::UP.decode(),
        Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MAX)
    );
    assert_eq!(OctDirection::encode(Direction::ZERO), OctDirection::UP);
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

mod digest_interop {
    use core::hash::Hash as _;

    use corvid_fixed::I16F16;
    use corvid_hash::{Digest, Hasher, digest};

    use super::{Direction, FinePoint, GlobalFinePoint, GlobalPoint, I24F8, Signed32};

    /// A point absorbs its three components in `x`, `y`, `z` order, each
    /// through its own encoding, and nothing else — no length, because the
    /// arity is in the type.
    fn corner() -> Digest {
        let mut hasher = Hasher::new();
        for bits in [1, 2, 3] {
            I24F8::from_bits(bits).hash(&mut hasher);
        }
        hasher.digest()
    }

    #[test]
    fn a_point_absorbs_its_components_and_no_length() {
        let point = GlobalPoint::new(
            I24F8::from_bits(1),
            I24F8::from_bits(2),
            I24F8::from_bits(3),
        );
        assert_eq!(corner(), digest(&point));
    }

    #[test]
    fn the_components_are_absorbed_in_order() {
        // A point that absorbed its components as an unordered set, or that
        // dropped one, would let two different positions agree on a mark.
        let one = I16F16::from_bits(1);
        let x = FinePoint::new(one, I16F16::ZERO, I16F16::ZERO);
        let y = FinePoint::new(I16F16::ZERO, one, I16F16::ZERO);
        let z = FinePoint::new(I16F16::ZERO, I16F16::ZERO, one);
        assert_ne!(digest(&x), digest(&y));
        assert_ne!(digest(&y), digest(&z));
        assert_ne!(digest(&x), digest(&FinePoint::ZERO));
    }

    #[test]
    fn a_denormal_component_digests_as_the_direction_it_denotes() {
        // `Direction` already folds `Signed32`'s second encoding of `-1.0` for
        // `Eq` and for `Hash`; the digest has to fold in the same place, or two
        // directions that compare equal would produce different marks.
        let canonical = Direction::from_array([Signed32::MIN, Signed32::ZERO, Signed32::ZERO]);
        let denormal = Direction::from_array([
            Signed32::from_bits(i32::MIN),
            Signed32::ZERO,
            Signed32::ZERO,
        ]);
        assert_eq!(canonical, denormal);
        assert_eq!(digest(&canonical), digest(&denormal));
    }

    #[test]
    fn a_packed_normal_digests_as_the_direction_it_denotes() {
        use super::{OctDirection, Signed8};

        // Same argument as `Direction`'s, at the narrower width: the two
        // spellings of `-1.0` are one component, so they are one normal, so they
        // are one mark.
        let canonical = OctDirection::new(Signed8::from_bits(-127), Signed8::from_bits(40));
        let denormal = OctDirection::new(Signed8::from_bits(-128), Signed8::from_bits(40));
        assert_eq!(canonical, denormal);
        assert_eq!(digest(&canonical), digest(&denormal));

        // And the two components are absorbed in order, so a swapped pair —
        // which names a different direction — is a different mark.
        let swapped = OctDirection::new(Signed8::from_bits(40), Signed8::from_bits(-127));
        assert_ne!(canonical.decode(), swapped.decode());
        assert_ne!(digest(&canonical), digest(&swapped));
    }

    #[test]
    fn every_point_type_is_digestible() {
        // Every point type hashes, and none of them hashes to nothing.
        assert_ne!(digest(&GlobalFinePoint::ZERO), Digest::ZERO);
        assert_ne!(digest(&GlobalPoint::ZERO), Digest::ZERO);
        assert_ne!(digest(&FinePoint::ZERO), Digest::ZERO);
        assert_ne!(
            digest(&Direction::from_array([Signed32::ZERO; 3])),
            Digest::ZERO
        );
        assert_ne!(digest(&super::OctDirection::UP), Digest::ZERO);
    }
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

    // The packed normal is the one whose `bytes_of` a vertex buffer actually
    // holds, so its two bytes are the two components in order and nothing else.
    let normal = OctDirection::new(Signed8::from_bits(40), Signed8::from_bits(-3));
    assert_eq!(bytemuck::bytes_of(&normal), &[40, 0xfd]);
    assert_eq!(
        <OctDirection as bytemuck::Zeroable>::zeroed(),
        OctDirection::UP
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

    // The packed normal is a two-element array for the same reason.
    let normal = OctDirection::new(Signed8::from_bits(40), Signed8::from_bits(-3));
    assert_eq!(serde_json::to_string(&normal).unwrap(), "[40,-3]");
    assert_eq!(
        serde_json::from_str::<OctDirection>("[40,-3]").unwrap(),
        normal
    );

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
