//! Layout guarantees, the wire format, and the optional integrations.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use core::mem::{align_of, size_of};

use corvid_rotation::Rotation;

use corvid_transform::{GlobalFineTransform, Transform};

use corvid_vector::GlobalPoint;
#[test]
fn the_wire_sizes_are_what_the_docs_claim() {
    assert_eq!((size_of::<Transform>(), align_of::<Transform>()), (16, 4));
    assert_eq!(
        (
            size_of::<GlobalFineTransform>(),
            align_of::<GlobalFineTransform>()
        ),
        (32, 8)
    );
}

#[test]
fn equal_transforms_hash_equally() {
    use std::collections::HashSet;

    let mut set = HashSet::new();
    set.insert(Transform::IDENTITY);
    set.insert(Transform::new(GlobalPoint::ZERO, Rotation::IDENTITY));
    assert_eq!(set.len(), 1);
}

#[cfg(feature = "serde")]
#[test]
fn a_transform_is_three_scalars_and_one_integer_on_the_wire() {
    use corvid_fixed::{I24F8, I48F16};
    use corvid_rotation::FineRotation;
    use corvid_vector::GlobalFinePoint;
    // The 16 B and 32 B figures have to mean something over the wire, so assert
    // the serialized form rather than only that a round trip succeeds.
    let t = Transform::new(
        GlobalPoint::new(
            I24F8::from_bits(1),
            I24F8::from_bits(2),
            I24F8::from_bits(3),
        ),
        Rotation::from_bits(7),
    );
    assert_eq!(
        serde_json::to_string(&t).unwrap(),
        r#"{"position":[1,2,3],"rotation":7}"#
    );
    assert_eq!(
        serde_json::from_str::<Transform>(r#"{"position":[1,2,3],"rotation":7}"#).unwrap(),
        t
    );

    let f = GlobalFineTransform::new(
        GlobalFinePoint::splat(I48F16::from_bits(1 << 40)),
        FineRotation::from_bits(9),
    );
    let text = serde_json::to_string(&f).unwrap();
    assert_eq!(
        text,
        r#"{"position":[1099511627776,1099511627776,1099511627776],"rotation":9}"#
    );
    assert_eq!(
        serde_json::from_str::<GlobalFineTransform>(&text).unwrap(),
        f
    );
}

#[cfg(feature = "bytemuck")]
#[test]
fn transforms_are_plain_old_data() {
    let t = Transform::IDENTITY;
    let bytes: &[u8] = bytemuck::bytes_of(&t);
    assert_eq!(bytes.len(), 16);
    assert_eq!(bytemuck::pod_read_unaligned::<Transform>(bytes), t);

    let f = GlobalFineTransform::IDENTITY;
    assert_eq!(bytemuck::bytes_of(&f).len(), 32);
    assert_eq!(
        bytemuck::pod_read_unaligned::<GlobalFineTransform>(bytemuck::bytes_of(&f)),
        f
    );
}

mod digest_interop {
    use core::hash::Hash as _;

    use corvid_fixed::{Angle16, I24F8};
    use corvid_hash::{Digest, Hasher, digest};
    use corvid_rotation::{Basis, FineRotation, Rotation, Versor};
    use corvid_transform::{GlobalFineTransform, Transform};
    use corvid_vector::{Direction, FinePoint, GlobalFinePoint, GlobalPoint};
    #[test]
    fn a_transform_absorbs_its_position_and_then_its_rotation() {
        // Absorbed by hand in declaration order, with no discriminant and no
        // length, because both fields are always present and both are fixed
        // width. Nothing type-checks that order, so this is what holds it.
        let position = GlobalPoint::new(
            I24F8::from_bits(1),
            I24F8::from_bits(2),
            I24F8::from_bits(3),
        );
        let rotation = Rotation::from_bits(7);

        let mut hasher = Hasher::new();
        position.hash(&mut hasher);
        rotation.hash(&mut hasher);

        assert_eq!(hasher.digest(), digest(&Transform::new(position, rotation)));
    }

    #[test]
    fn the_position_and_the_rotation_both_move_the_digest() {
        let moved = Transform::new(
            GlobalPoint::new(I24F8::from_bits(1), I24F8::ZERO, I24F8::ZERO),
            Rotation::IDENTITY,
        );
        let turned = Transform::new(GlobalPoint::ZERO, Rotation::from_bits(1));
        assert_ne!(digest(&Transform::IDENTITY), digest(&moved));
        assert_ne!(digest(&Transform::IDENTITY), digest(&turned));
        assert_ne!(digest(&moved), digest(&turned));
    }

    #[test]
    fn every_layer_below_hashes_too() {
        // A transform is built out of three crates' types, and a game hashing
        // one hashes all of them. None of these may digest to nothing.
        assert_ne!(digest(&Angle16::QUARTER_TURN), Digest::ZERO);
        assert_ne!(digest(&I24F8::ONE), Digest::ZERO);
        assert_ne!(digest(&GlobalPoint::ZERO), Digest::ZERO);
        assert_ne!(digest(&GlobalFinePoint::ZERO), Digest::ZERO);
        assert_ne!(digest(&FinePoint::ZERO), Digest::ZERO);
        assert_ne!(digest(&Direction::ZERO), Digest::ZERO);
        assert_ne!(digest(&Rotation::IDENTITY), Digest::ZERO);
        assert_ne!(digest(&FineRotation::IDENTITY), Digest::ZERO);
        assert_ne!(digest(&Versor::IDENTITY), Digest::ZERO);
        assert_ne!(digest(&Basis::IDENTITY), Digest::ZERO);
        assert_ne!(digest(&GlobalFineTransform::IDENTITY), Digest::ZERO);
    }
}

#[test]
fn the_crate_re_exports_the_whole_stack() {
    // One name for downstream code to depend on.
    let _: corvid_fixed::I24F8 = corvid_fixed::I24F8::ONE;
    let _: corvid_fixed::Angle32 = corvid_fixed::Angle32::QUARTER_TURN;
    let _: corvid_rotation::Basis = corvid_rotation::Basis::IDENTITY;
    let _: corvid_rotation::Versor = corvid_rotation::Versor::IDENTITY;
    let _: corvid_vector::Direction = corvid_vector::Direction::ZERO;
    let _: corvid_vector::FinePoint = corvid_vector::FinePoint::ZERO;
    let _ = corvid_fixed::I0F8::ZERO;
    let _ = corvid_vector::GlobalPoint::ZERO;
    let _ = corvid_rotation::Rotation::IDENTITY;
}
